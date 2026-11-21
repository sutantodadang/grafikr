package grafikr

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path"
	"path/filepath"
	"runtime"
	"strings"
)

type runtimeFile struct {
	Path   string `json:"path"`
	Size   int64  `json:"size"`
	SHA256 string `json:"sha256"`
	Role   string `json:"role"`
}

type runtimeManifest struct {
	EngineVersion      string        `json:"engine_version"`
	Target             string        `json:"target"`
	DevelopmentFixture bool          `json:"development_fixture"`
	Files              []runtimeFile `json:"files"`
}

var runtimeTargets = map[string]string{
	"windows/amd64": "x86_64-pc-windows-msvc",
	"windows/arm64": "aarch64-pc-windows-msvc",
	"linux/amd64":   "x86_64-unknown-linux-gnu",
	"linux/arm64":   "aarch64-unknown-linux-gnu",
	"darwin/amd64":  "x86_64-apple-darwin",
	"darwin/arm64":  "aarch64-apple-darwin",
}

// InstallRuntime validates a bundle and copies it atomically into cacheDir.
func InstallRuntime(bundleDir, cacheDir string) (string, error) {
	target, ok := runtimeTargets[runtime.GOOS+"/"+runtime.GOARCH]
	if !ok {
		return "", fmt.Errorf("Grafikr runtime is unavailable for %s/%s", runtime.GOOS, runtime.GOARCH)
	}
	bundle, err := filepath.Abs(bundleDir)
	if err != nil {
		return "", err
	}
	manifest, err := verifyRuntime(bundle, target)
	if err != nil {
		return "", err
	}
	cache, err := filepath.Abs(cacheDir)
	if err != nil {
		return "", err
	}
	root := filepath.Join(cache, target, manifest.EngineVersion)
	if existing, err := verifyRuntime(root, target); err == nil {
		return filepath.Join(root, hostRelativePath(existing)), nil
	} else if !errors.Is(err, os.ErrNotExist) {
		return "", err
	}
	if err := os.MkdirAll(filepath.Dir(root), 0o755); err != nil {
		return "", err
	}
	stage, err := os.MkdirTemp(filepath.Dir(root), ".grafikr-runtime-")
	if err != nil {
		return "", err
	}
	renamed := false
	defer func() {
		if !renamed {
			_ = os.RemoveAll(stage)
		}
	}()
	for _, file := range manifest.Files {
		rel, _ := safeRuntimePath(file.Path)
		source := filepath.Join(bundle, rel)
		destination := filepath.Join(stage, rel)
		if err := os.MkdirAll(filepath.Dir(destination), 0o755); err != nil {
			return "", err
		}
		if err := copyRuntimeFile(source, destination, file.Role == "host"); err != nil {
			return "", err
		}
	}
	if err := copyRuntimeFile(filepath.Join(bundle, "manifest.json"), filepath.Join(stage, "manifest.json"), false); err != nil {
		return "", err
	}
	if _, err := verifyRuntime(stage, target); err != nil {
		return "", err
	}
	if err := os.Rename(stage, root); err != nil {
		if _, existingErr := verifyRuntime(root, target); existingErr == nil {
			return filepath.Join(root, hostRelativePath(manifest)), nil
		}
		return "", err
	}
	renamed = true
	return filepath.Join(root, hostRelativePath(manifest)), nil
}

// StartWithRuntime installs a previously unpacked bundle before starting its host.
func StartWithRuntime(bundleDir, cacheDir string) (*App, error) {
	host, err := InstallRuntime(bundleDir, cacheDir)
	if err != nil {
		return nil, err
	}
	return Start(host)
}

func verifyRuntime(root, target string) (runtimeManifest, error) {
	var manifest runtimeManifest
	rootInfo, err := os.Lstat(root)
	if err != nil {
		return manifest, err
	}
	if !rootInfo.IsDir() || rootInfo.Mode()&os.ModeSymlink != 0 {
		return manifest, fmt.Errorf("runtime bundle directory is unsafe")
	}
	manifestPath := filepath.Join(root, "manifest.json")
	manifestInfo, err := os.Lstat(manifestPath)
	if err != nil {
		return manifest, err
	}
	if !manifestInfo.Mode().IsRegular() || manifestInfo.Mode()&os.ModeSymlink != 0 || manifestInfo.Size() > 1<<20 {
		return manifest, fmt.Errorf("runtime manifest is unsafe")
	}
	data, err := os.ReadFile(manifestPath)
	if err != nil {
		return manifest, err
	}
	if err := json.Unmarshal(data, &manifest); err != nil {
		return manifest, fmt.Errorf("runtime manifest is invalid: %w", err)
	}
	if manifest.EngineVersion == "" || manifest.Target != target || manifest.DevelopmentFixture || len(manifest.Files) == 0 {
		return manifest, fmt.Errorf("runtime manifest target or release metadata is invalid")
	}
	hasHost := false
	for _, file := range manifest.Files {
		rel, err := safeRuntimePath(file.Path)
		if err != nil {
			return manifest, err
		}
		full := filepath.Join(root, rel)
		for parent := filepath.Dir(full); parent != root && strings.HasPrefix(parent, root+string(os.PathSeparator)); parent = filepath.Dir(parent) {
			info, err := os.Lstat(parent)
			if err != nil {
				return manifest, err
			}
			if !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
				return manifest, fmt.Errorf("runtime path is unsafe: %s", file.Path)
			}
		}
		info, err := os.Lstat(full)
		if err != nil {
			return manifest, err
		}
		if !info.Mode().IsRegular() || info.Mode()&os.ModeSymlink != 0 || info.Size() != file.Size {
			return manifest, fmt.Errorf("runtime file is unsafe or changed: %s", file.Path)
		}
		input, err := os.Open(full)
		if err != nil {
			return manifest, err
		}
		hasher := sha256.New()
		_, hashErr := io.Copy(hasher, input)
		closeErr := input.Close()
		if hashErr != nil {
			return manifest, hashErr
		}
		if closeErr != nil {
			return manifest, closeErr
		}
		if hex.EncodeToString(hasher.Sum(nil)) != file.SHA256 {
			return manifest, fmt.Errorf("runtime checksum does not match: %s", file.Path)
		}
		if file.Role == "host" {
			hasHost = true
		}
	}
	if !hasHost {
		return manifest, fmt.Errorf("runtime manifest has no host executable")
	}
	return manifest, nil
}

func safeRuntimePath(value string) (string, error) {
	if value == "" || strings.Contains(value, "\\") || path.IsAbs(value) || path.Clean(value) != value {
		return "", fmt.Errorf("unsafe runtime path: %q", value)
	}
	for _, part := range strings.Split(value, "/") {
		if part == "" || part == "." || part == ".." {
			return "", fmt.Errorf("unsafe runtime path: %q", value)
		}
	}
	return filepath.FromSlash(value), nil
}

func copyRuntimeFile(source, destination string, executable bool) error {
	input, err := os.Open(source)
	if err != nil {
		return err
	}
	defer input.Close()
	info, err := input.Stat()
	if err != nil {
		return err
	}
	if !info.Mode().IsRegular() {
		return fmt.Errorf("runtime file is not regular: %s", source)
	}
	mode := info.Mode().Perm()
	if executable && runtime.GOOS != "windows" {
		mode |= 0o755
	}
	output, err := os.OpenFile(destination, os.O_CREATE|os.O_EXCL|os.O_WRONLY, mode)
	if err != nil {
		return err
	}
	_, copyErr := io.Copy(output, input)
	closeErr := output.Close()
	if copyErr != nil {
		return copyErr
	}
	return closeErr
}

func hostRelativePath(manifest runtimeManifest) string {
	for _, file := range manifest.Files {
		if file.Role == "host" {
			rel, _ := safeRuntimePath(file.Path)
			return rel
		}
	}
	return ""
}

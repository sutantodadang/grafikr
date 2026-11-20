<?php
declare(strict_types=1);
namespace Grafikr;

final class Runtime
{
    private const TARGETS = [
        'windows-x64' => 'x86_64-pc-windows-msvc',
        'windows-arm64' => 'aarch64-pc-windows-msvc',
        'linux-x64' => 'x86_64-unknown-linux-gnu',
        'linux-arm64' => 'aarch64-unknown-linux-gnu',
        'macos-x64' => 'x86_64-apple-darwin',
        'macos-arm64' => 'aarch64-apple-darwin',
    ];

    public static function hostPath(?string $bundle = null): string
    {
        $target = self::target();
        if ($bundle === null) {
            $package = 'grafikr/runtime-' . self::slug($target);
            if (!class_exists(\Composer\InstalledVersions::class)) {
                throw new BridgeException('runtime_missing', 'Install the Composer runtime package matching this platform');
            }
            $installPath = \Composer\InstalledVersions::getInstallPath($package);
            if (!is_string($installPath) || $installPath === '') {
                throw new BridgeException('runtime_missing', "Composer runtime package {$package} is not installed");
            }
            $bundle = $installPath . DIRECTORY_SEPARATOR . 'runtime';
        }
        if (is_link($bundle) || !is_dir($bundle)) {
            throw new BridgeException('runtime_path', 'Grafikr runtime bundle directory is missing or unsafe');
        }
        $root = realpath($bundle);
        $manifestPath = $root === false ? '' : $root . DIRECTORY_SEPARATOR . 'manifest.json';
        $manifestStat = @lstat($manifestPath);
        if ($root === false || is_link($manifestPath) || $manifestStat === false || ($manifestStat['mode'] & 0170000) !== 0100000 || $manifestStat['size'] > 1048576) {
            throw new BridgeException('runtime_manifest', 'Grafikr runtime manifest is missing or unsafe');
        }
        try {
            $manifest = json_decode((string)file_get_contents($manifestPath), true, 64, JSON_THROW_ON_ERROR);
        } catch (\JsonException $error) {
            throw new BridgeException('runtime_manifest', 'Grafikr runtime manifest is invalid');
        }
        if (!is_array($manifest) || ($manifest['target'] ?? null) !== $target || ($manifest['engine_version'] ?? null) !== App::SDK_VERSION || ($manifest['development_fixture'] ?? null) !== false || !is_array($manifest['files'] ?? null) || $manifest['files'] === []) {
            throw new BridgeException('runtime_target', 'Grafikr runtime target or release metadata does not match this PHP package');
        }
        $host = null;
        foreach ($manifest['files'] as $entry) {
            if (!is_array($entry) || !is_string($entry['path'] ?? null)) {
                throw new BridgeException('runtime_manifest', 'Grafikr runtime manifest entry is invalid');
            }
            $relative = $entry['path'];
            $parts = explode('/', $relative);
            if ($relative === '' || str_contains($relative, '\\') || str_starts_with($relative, '/') || preg_match('/^[A-Za-z]:/', $relative) || in_array('', $parts, true) || in_array('.', $parts, true) || in_array('..', $parts, true)) {
                throw new BridgeException('runtime_path', 'Grafikr runtime manifest contains an unsafe path');
            }
            $path = $root . DIRECTORY_SEPARATOR . implode(DIRECTORY_SEPARATOR, $parts);
            $parent = $root;
            foreach (array_slice($parts, 0, -1) as $part) {
                $parent .= DIRECTORY_SEPARATOR . $part;
                if (is_link($parent) || !is_dir($parent)) {
                    throw new BridgeException('runtime_path', 'Grafikr runtime path contains an unsafe directory');
                }
            }
            $stat = @lstat($path);
            if ($stat === false || is_link($path) || ($stat['mode'] & 0170000) !== 0100000 || $stat['size'] !== ($entry['size'] ?? null) || !is_string($entry['sha256'] ?? null) || !hash_equals($entry['sha256'], (string)hash_file('sha256', $path))) {
                throw new BridgeException('runtime_checksum', "Grafikr runtime file failed verification: {$relative}");
            }
            if (($entry['role'] ?? null) === 'host') $host = $path;
        }
        if ($host === null) {
            throw new BridgeException('runtime_manifest', 'Grafikr runtime manifest has no host executable');
        }
        return $host;
    }

    private static function target(): string
    {
        $os = match (PHP_OS_FAMILY) {
            'Windows' => 'windows',
            'Linux' => 'linux',
            'Darwin' => 'macos',
            default => null,
        };
        $machine = strtolower(php_uname('m'));
        $architecture = in_array($machine, ['x86_64', 'amd64', 'x64'], true) ? 'x64' : (in_array($machine, ['aarch64', 'arm64'], true) ? 'arm64' : null);
        $slug = $os === null || $architecture === null ? null : "{$os}-{$architecture}";
        if ($slug === null || !isset(self::TARGETS[$slug])) {
            throw new BridgeException('runtime_target', 'Grafikr has no runtime for this PHP platform');
        }
        return self::TARGETS[$slug];
    }

    private static function slug(string $target): string
    {
        $slug = array_search($target, self::TARGETS, true);
        if (!is_string($slug)) throw new BridgeException('runtime_target', 'Grafikr has no package for this runtime target');
        return $slug;
    }
}

require "digest"
require "json"
require "rubygems"
require "rbconfig"

module Grafikr
  module Runtime
    TARGETS = {
      ["windows", "x86_64"] => "x86_64-pc-windows-msvc",
      ["windows", "aarch64"] => "aarch64-pc-windows-msvc",
      ["linux", "x86_64"] => "x86_64-unknown-linux-gnu",
      ["linux", "aarch64"] => "aarch64-unknown-linux-gnu",
      ["macos", "x86_64"] => "x86_64-apple-darwin",
      ["macos", "aarch64"] => "aarch64-apple-darwin"
    }.freeze

    def self.host_path(bundle = nil)
      target = current_target
      root = bundle || File.join(Gem::Specification.find_by_name("grafikr-ui").full_gem_path, "runtime")
      raise Error.new("runtime_path", "Grafikr runtime directory is unsafe") if File.symlink?(root) || !File.directory?(root)
      root = File.realpath(root)
      manifest_path = File.join(root, "manifest.json")
      manifest_stat = File.lstat(manifest_path)
      raise Error.new("runtime_manifest", "Grafikr runtime manifest is unsafe") if manifest_stat.symlink? || !manifest_stat.file? || manifest_stat.size > 1_048_576
      manifest = JSON.parse(File.binread(manifest_path))
      unless manifest["target"] == target && manifest["engine_version"] == SDK_VERSION && manifest["development_fixture"] == false
        raise Error.new("runtime_target", "Grafikr runtime target or release version does not match this Ruby gem")
      end
      files = manifest["files"]
      raise Error.new("runtime_manifest", "Grafikr runtime manifest has no files") unless files.is_a?(Array) && !files.empty?
      host = nil
      files.each do |entry|
        relative = entry["path"]
        unless relative.is_a?(String) && !relative.empty? && !relative.include?("\\") && !relative.start_with?("/") && relative.split("/").none? { |part| part.empty? || part == "." || part == ".." }
          raise Error.new("runtime_path", "Grafikr runtime manifest contains an unsafe path")
        end
        path = File.expand_path(relative, root)
        unless path.start_with?(root + File::SEPARATOR)
          raise Error.new("runtime_path", "Grafikr runtime manifest path escapes its package")
        end
        parent = root
        relative.split("/")[0...-1].each do |part|
          parent = File.join(parent, part)
          raise Error.new("runtime_path", "Grafikr runtime path contains a symlink") if File.symlink?(parent)
        end
        stat = File.lstat(path)
        unless stat.file? && !stat.symlink? && stat.size == entry["size"] && Digest::SHA256.file(path).hexdigest == entry["sha256"]
          raise Error.new("runtime_checksum", "Grafikr runtime file failed verification: #{relative}")
        end
        host = path if entry["role"] == "host"
      end
      raise Error.new("runtime_manifest", "Grafikr runtime manifest has no host executable") unless host
      host
    rescue JSON::ParserError, Errno::ENOENT, Errno::EACCES => error
      raise Error.new("runtime_manifest", "Grafikr runtime bundle is missing or invalid: #{error.message}")
    rescue Gem::LoadError => error
      raise Error.new("runtime_missing", "Grafikr Ruby platform gem is not installed: #{error.message}")
    end

    def self.current_target
      os = RbConfig::CONFIG["host_os"]
      cpu = RbConfig::CONFIG["host_cpu"]
      operating_system = os.match?(/mswin|mingw/i) ? "windows" : os.match?(/darwin/i) ? "macos" : os.match?(/linux/i) ? "linux" : nil
      architecture = cpu.match?(/x86_64|amd64|x64/i) ? "x86_64" : cpu.match?(/aarch64|arm64/i) ? "aarch64" : nil
      TARGETS.fetch([operating_system, architecture]) { raise Error.new("runtime_target", "Grafikr has no runtime for this Ruby platform") }
    end
    private_class_method :current_target
  end
end

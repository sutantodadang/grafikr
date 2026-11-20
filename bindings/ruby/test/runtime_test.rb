require "minitest/autorun"
require "tmpdir"
require "fileutils"
require "digest"
require "json"
require_relative "../lib/grafikr"

class RuntimeTest < Minitest::Test
  def with_bundle
    Dir.mktmpdir("grafikr-runtime-test") do |root|
      FileUtils.mkdir_p(File.join(root, "bin"))
      host = File.join(root, "bin", "grafikr-host")
      File.binwrite(host, "runtime host fixture")
      target = Grafikr::Runtime.send(:current_target)
      manifest = {
        "engine_version" => Grafikr::SDK_VERSION,
        "target" => target,
        "development_fixture" => false,
        "files" => [{
          "path" => "bin/grafikr-host",
          "size" => File.size(host),
          "sha256" => Digest::SHA256.file(host).hexdigest,
          "role" => "host"
        }]
      }
      File.write(File.join(root, "manifest.json"), JSON.generate(manifest))
      yield root, host, manifest
    end
  end

  def test_bundle_returns_verified_absolute_host_path
    with_bundle do |root, host, _manifest|
      assert_equal host, Grafikr::Runtime.host_path(root)
      assert_equal File.absolute_path(host), host
    end
  end

  def test_client_entry_point_aliases_the_resource_safe_app
    assert_same Grafikr::App, Grafikr::Client
    assert_respond_to Grafikr::Client, :start
    assert_respond_to Grafikr::Client, :open
  end

  def test_checksum_mismatch_rejects_tampered_runtime
    with_bundle do |root, host, _manifest|
      File.binwrite(host, "changed host")
      error = assert_raises(Grafikr::Error) { Grafikr::Runtime.host_path(root) }
      assert_equal "runtime_checksum", error.code
    end
  end

  def test_manifest_traversal_is_rejected
    with_bundle do |root, _host, manifest|
      manifest["files"][0]["path"] = "../outside"
      File.write(File.join(root, "manifest.json"), JSON.generate(manifest))
      error = assert_raises(Grafikr::Error) { Grafikr::Runtime.host_path(root) }
      assert_equal "runtime_path", error.code
    end
  end

  def test_wrong_platform_bundle_is_rejected
    with_bundle do |root, _host, manifest|
      manifest["target"] = "aarch64-apple-darwin"
      File.write(File.join(root, "manifest.json"), JSON.generate(manifest))
      error = assert_raises(Grafikr::Error) { Grafikr::Runtime.host_path(root) }
      assert_equal "runtime_target", error.code
    end
  end
end

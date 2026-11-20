import unittest

from scripts.verify_node_package import validate_optional_dependencies


class NodeOptionalDependencyTests(unittest.TestCase):
    def test_all_six_platform_runtime_packages_are_optional_dependencies(self):
        metadata = {
            "version": "1.1.0",
            "optionalDependencies": {
                f"@grafikr/runtime-{target}": "1.1.0"
                for target in (
                    "win32-x64", "win32-arm64", "linux-x64", "linux-arm64",
                    "darwin-x64", "darwin-arm64",
                )
            }
        }
        validate_optional_dependencies(metadata)

    def test_missing_or_wrongly_versioned_runtime_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "runtime-win32-x64"):
            validate_optional_dependencies({"optionalDependencies": {}})


if __name__ == "__main__":
    unittest.main()

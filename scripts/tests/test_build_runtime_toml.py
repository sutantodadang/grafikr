import importlib
import sys
import unittest
from unittest.mock import patch

import scripts.build_runtime as build_runtime


class BuildRuntimeTomlTests(unittest.TestCase):
    def test_python_310_uses_the_pinned_tomli_fallback(self):
        with patch.dict(sys.modules, {"tomllib": None}):
            module = importlib.reload(build_runtime)

        try:
            self.assertEqual(module.tomllib.__name__, "tomli")
        finally:
            importlib.reload(module)


if __name__ == "__main__":
    unittest.main()

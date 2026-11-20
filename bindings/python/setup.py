from setuptools import setup
from setuptools.command.bdist_wheel import bdist_wheel


class PlatformWheel(bdist_wheel):
    def get_tag(self):
        return "py3", "none", self.plat_name.replace("-", "_")


setup(cmdclass={"bdist_wheel": PlatformWheel})

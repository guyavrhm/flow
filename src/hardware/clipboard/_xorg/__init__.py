import subprocess

from src.files import XCOPY, XPASTE


class LinuxClipboard:
    """
    Linux Clipboard api
    """

    @staticmethod
    def set_files(files: list):
        """
        file sharing not supported on linux
        """
        LinuxClipboard.set_text(str(files))

    @staticmethod
    def data():
        """
        Returns clipboard data.
        """
        try:
            out = subprocess.check_output(XPASTE, stderr=subprocess.STDOUT).decode()
        except subprocess.CalledProcessError:
            out = 'unknown format'
        return out

    @staticmethod
    def set_text(data: str):
        """
        Sets plain textf to the clipboard.
        """
        subprocess.call([XCOPY, data])

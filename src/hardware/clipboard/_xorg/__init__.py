import subprocess

from src.files import XCOPY, XPASTE


class LinuxClipboard:
    """
    Linux Clipboard api
    """

    @staticmethod
    def set_files(files):
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
            out = subprocess.check_output(XPASTE, stderr=subprocess.STDOUT, shell=True).decode()
        except subprocess.CalledProcessError:
            out = 'unknown format'
        return out

    @staticmethod
    def set_text(data):
        """
        Sets plain text to the clipboard.
        """
        data = data.replace('"', '\\"')
        subprocess.call(f'{XCOPY} "{data}"', shell=True)

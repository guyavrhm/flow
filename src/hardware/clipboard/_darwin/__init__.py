import subprocess

from src.files import FILE2CLIP_MAC, GET_FILES_MAC
from src.hardware.clipboard._base import BaseClipboard


class MacOSClipboard(BaseClipboard):
    """
    MacOS Clipboard api
    """

    @staticmethod
    def set_files(files: list):
        """
        Sets given file paths to the clipboard.
        """
        subprocess.Popen([FILE2CLIP_MAC] + files)

    @staticmethod
    def data():
        """
        Returns clipboard data.
        """
        files = subprocess.check_output(GET_FILES_MAC).decode().strip()
        if files:
            out = tuple(files.split("\n"))
        else:
            out = subprocess.check_output(['pbpaste']).decode('utf-8')
        return out

    @staticmethod
    def set_text(data: str):
        """
        Sets plain text to the clipboard.
        """
        p = subprocess.Popen(['pbcopy'], stdin=subprocess.PIPE)
        p.communicate(input=data.encode('utf-8'))

import subprocess

from src.files import FILE2CLIP_MAC, GET_FILES_MAC, MCOPY, MPASTE


class MacOSClipboard:
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
        files = subprocess.check_output(GET_FILES_MAC).decode()
        if len(files) != 1:
            out = tuple(files.split("\n")[:-1])
        else:
            out = subprocess.check_output(MPASTE).decode()
        return out

    @staticmethod
    def set_text(data: str):
        """
        Sets plain text to the clipboard.
        """
        subprocess.call([MCOPY, data])

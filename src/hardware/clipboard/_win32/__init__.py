import subprocess
import win32clipboard

from src.files import FILE2CLIP_WIN


class WindowsClipboard:
    """
    Windows Clipboard api
    """

    @staticmethod
    def set_files(files):
        """
        Sets given file paths to the clipboard.
        """
        subprocess.Popen([FILE2CLIP_WIN] + files)

    @staticmethod
    def data():
        """
        Returns clipboard data.
        """
        win32clipboard.OpenClipboard()
        try:
            try:
                data = win32clipboard.GetClipboardData()
            except TypeError:
                data = win32clipboard.GetClipboardData(win32clipboard.CF_HDROP)
        except TypeError:
            data = 'unknown format'
        win32clipboard.CloseClipboard()
        return data

    @staticmethod
    def set_text(data):
        """
        Sets plain text to the clipboard.
        """
        win32clipboard.OpenClipboard()
        win32clipboard.EmptyClipboard()
        win32clipboard.SetClipboardText(data)
        win32clipboard.CloseClipboard()

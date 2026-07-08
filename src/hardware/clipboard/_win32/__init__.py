import subprocess
import win32clipboard
import logging
import time

from src.files import FILE2CLIP_WIN
from src.hardware.clipboard._base import BaseClipboard

logger = logging.getLogger(__name__)


class WindowsClipboard(BaseClipboard):
    """
    Windows Clipboard api
    """

    @staticmethod
    def set_files(files: list):
        """
        Sets given file paths to the clipboard.
        """
        if len(files) != 0:
            subprocess.Popen([FILE2CLIP_WIN] + files)

    @staticmethod
    def data():
        """
        Returns clipboard data.
        """
        retries = 10
        delay = 0.05  # 50ms
        for i in range(retries):
            try:
                win32clipboard.OpenClipboard()
                break
            except Exception as e:
                if i == retries - 1:
                    logger.debug("Failed to open Windows clipboard after retries: %s", e)
                    return 'unknown format'
                time.sleep(delay)
        try:
            try:
                data = win32clipboard.GetClipboardData()
            except TypeError:
                try:
                    data = win32clipboard.GetClipboardData(win32clipboard.CF_HDROP)
                except TypeError:
                    data = 'unknown format'
        except Exception as e:
            logger.debug("Failed to get Windows clipboard data: %s", e)
            data = 'unknown format'
        finally:
            try:
                win32clipboard.CloseClipboard()
            except Exception:
                pass
        return data

    @staticmethod
    def set_text(data: str):
        """
        Sets plain text to the clipboard.
        """
        retries = 10
        delay = 0.05  # 50ms
        for i in range(retries):
            try:
                win32clipboard.OpenClipboard()
                break
            except Exception as e:
                if i == retries - 1:
                    logger.debug("Failed to open Windows clipboard to set text after retries: %s", e)
                    return
                time.sleep(delay)
        try:
            win32clipboard.EmptyClipboard()
            win32clipboard.SetClipboardText(data)
        except Exception as e:
            logger.debug("Failed to set Windows clipboard text: %s", e)
        finally:
            try:
                win32clipboard.CloseClipboard()
            except Exception:
                pass

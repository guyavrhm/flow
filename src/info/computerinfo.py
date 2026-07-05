"""
File containing static methods used to obtain machine information.
"""

from sys import platform
import socket
from PyQt5.QtWidgets import QApplication

WINDOWS = 'win32'
LINUX = 'linux'
MACOS = 'darwin'


def supported():
    """
    Returns True if computer is supported.
    """
    return platform == WINDOWS or platform == LINUX or platform == MACOS


def get_screeninfo():
    """
    Returns screen resolution of computer.
    Uses PyQt5's primary screen information.
    """
    app = QApplication.instance()
    if app:
        screen = app.primaryScreen()
        if screen:
            size = screen.size()
            return size.width(), size.height()
    # Fallback to standard HD resolution if QApplication is not instantiated
    return 1920, 1080


def get_ip():
    """
    Returns local IP of computer using standard socket connection routing.
    """
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    try:
        # Connect to a dummy address (doesn't send any packets) to determine local IP routing
        s.connect(('8.8.8.8', 80))
        ip = s.getsockname()[0]
    except Exception:
        ip = '127.0.0.1'
    finally:
        s.close()
    return ip

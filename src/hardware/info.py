"""
File containing static methods/constants used to obtain machine/platform information.
"""

from sys import platform
from PyQt5.QtWidgets import QApplication

WINDOWS = 'win32'
LINUX = 'linux'
MACOS = 'darwin'


def supported():
    """
    Returns True if computer is supported.
    """
    return platform == WINDOWS or platform == LINUX or platform == MACOS


def is_wayland():
    """
    Returns True if the Linux environment is running a Wayland session.
    """
    import os
    return platform == LINUX and (
        os.environ.get('XDG_SESSION_TYPE') == 'wayland' or
        'WAYLAND_DISPLAY' in os.environ
    )


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

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
    Uses PyQt5's primary screen information or X11 physical display dimensions on Linux.
    """
    from sys import platform
    if platform == 'linux':
        # On Linux/X11, we want the physical resolution for XTest simulation accuracy
        try:
            import ctypes
            x11 = ctypes.CDLL("libX11.so.6")
            x11.XOpenDisplay.argtypes = [ctypes.c_char_p]
            x11.XOpenDisplay.restype = ctypes.c_void_p
            x11.XDisplayWidth.argtypes = [ctypes.c_void_p, ctypes.c_int]
            x11.XDisplayWidth.restype = ctypes.c_int
            x11.XDisplayHeight.argtypes = [ctypes.c_void_p, ctypes.c_int]
            x11.XDisplayHeight.restype = ctypes.c_int
            x11.XCloseDisplay.argtypes = [ctypes.c_void_p]
            
            display = x11.XOpenDisplay(None)
            if display:
                width = x11.XDisplayWidth(display, 0)
                height = x11.XDisplayHeight(display, 0)
                x11.XCloseDisplay(display)
                return width, height
        except Exception:
            pass

    app = QApplication.instance()
    if app:
        screen = app.primaryScreen()
        if screen:
            size = screen.size()
            return size.width(), size.height()
    # Fallback to standard HD resolution if QApplication is not instantiated
    return 1920, 1080


def get_app_dir():
    """
    Returns the standard application config/data directory based on OS.
    """
    import os
    if platform == WINDOWS:
        return os.path.join(os.getenv('APPDATA'), 'flow')
    return os.path.expanduser('~/.flow')


def get_aes_extension():
    """
    Returns the shared library file extension for compiled AES code on this platform.
    """
    return 'dll' if platform == WINDOWS else 'so'

"""
Imports clipboard class according to operating system.
"""
import src.info.computerinfo as ci

if ci.platform == ci.WINDOWS:
    from ._win32 import WindowsClipboard as Clipboard
elif ci.platform == ci.MACOS:
    from ._darwin import MacOSClipboard as Clipboard
else:
    from ._xorg import LinuxClipboard as Clipboard

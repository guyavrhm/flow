"""
Constants for string key conversion +
keyboard functions
"""

import sys

# Platform-specific imports
if sys.platform == 'darwin':
    from ._darwin import MacOSKeyboardController as KeyboardController, MacOSKeyboardListener as KeyboardListener
elif sys.platform == 'win32':
    from ._win32 import WindowsKeyboardController as KeyboardController, WindowsKeyboardListener as KeyboardListener
else:
    from ._xorg import LinuxKeyboardController as KeyboardController, LinuxKeyboardListener as KeyboardListener


def key_from_str(key):
    """
    Parses a string representation of a key into a standardized format.
    Special keys remain prefixed with "Key.", while character keys are returned
    as a plain single-character string (without quotes).
    """
    if key.startswith("Key."):
        return key
    else:
        # Strip quotes if present (e.g. "'a'" -> 'a')
        if len(key) >= 3 and key[0] == "'" and key[-1] == "'":
            return key[1:-1]
        return key

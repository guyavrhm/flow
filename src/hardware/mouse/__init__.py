"""
Platform-appropriate mouse controllers and listeners.
"""

import sys

# Platform-specific imports
if sys.platform == 'darwin':
    from ._darwin import MacOSMouseController as MouseController, MacOSMouseListener as MouseListener
elif sys.platform == 'win32':
    from ._win32 import WindowsMouseController as MouseController, WindowsMouseListener as MouseListener
else:
    from ._xorg import LinuxMouseController as MouseController, LinuxMouseListener as MouseListener

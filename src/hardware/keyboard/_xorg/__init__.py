import ctypes
import ctypes.util
import logging
import threading

from src.hardware.keyboard._base import BaseKeyboardController, BaseKeyboardListener

logger = logging.getLogger(__name__)

x11_path = ctypes.util.find_library('X11')
if not x11_path:
    x11_path = 'libX11.so.6'
try:
    x11 = ctypes.CDLL(x11_path)
except Exception:
    x11 = None

xtst_path = ctypes.util.find_library('Xtst')
if not xtst_path:
    xtst_path = 'libXtst.so.6'
try:
    xtst = ctypes.CDLL(xtst_path)
except Exception:
    xtst = None

class XAnyEvent(ctypes.Structure):
    _fields_ = [
        ("type", ctypes.c_int),
        ("serial", ctypes.c_ulong),
        ("send_event", ctypes.c_int),
        ("display", ctypes.c_void_p),
        ("window", ctypes.c_ulong),
    ]

class XKeyEvent(ctypes.Structure):
    _fields_ = [
        ("type", ctypes.c_int),
        ("serial", ctypes.c_ulong),
        ("send_event", ctypes.c_int),
        ("display", ctypes.c_void_p),
        ("window", ctypes.c_ulong),
        ("root", ctypes.c_ulong),
        ("subwindow", ctypes.c_ulong),
        ("time", ctypes.c_ulong),
        ("x", ctypes.c_int),
        ("y", ctypes.c_int),
        ("x_root", ctypes.c_int),
        ("y_root", ctypes.c_int),
        ("state", ctypes.c_uint),
        ("keycode", ctypes.c_uint),
        ("same_screen", ctypes.c_int),
    ]

class XEvent(ctypes.Union):
    _fields_ = [
        ("type", ctypes.c_int),
        ("xany", XAnyEvent),
        ("xkey", XKeyEvent),
        ("pad", ctypes.c_char * 192),
    ]

if x11:
    x11.XOpenDisplay.argtypes = [ctypes.c_char_p]
    x11.XOpenDisplay.restype = ctypes.c_void_p
    x11.XCloseDisplay.argtypes = [ctypes.c_void_p]
    x11.XCloseDisplay.restype = ctypes.c_int
    x11.XDefaultRootWindow.argtypes = [ctypes.c_void_p]
    x11.XDefaultRootWindow.restype = ctypes.c_ulong
    x11.XGrabKeyboard.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_ulong]
    x11.XGrabKeyboard.restype = ctypes.c_int
    x11.XUngrabKeyboard.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
    x11.XUngrabKeyboard.restype = ctypes.c_int
    x11.XPending.argtypes = [ctypes.c_void_p]
    x11.XPending.restype = ctypes.c_int
    x11.XNextEvent.argtypes = [ctypes.c_void_p, ctypes.POINTER(XEvent)]
    x11.XNextEvent.restype = ctypes.c_int
    x11.XKeycodeToKeysym.argtypes = [ctypes.c_void_p, ctypes.c_ubyte, ctypes.c_int]
    x11.XKeycodeToKeysym.restype = ctypes.c_ulong
    x11.XKeysymToString.argtypes = [ctypes.c_ulong]
    x11.XKeysymToString.restype = ctypes.c_char_p
    x11.XStringToKeysym.argtypes = [ctypes.c_char_p]
    x11.XStringToKeysym.restype = ctypes.c_ulong
    x11.XKeysymToKeycode.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
    x11.XKeysymToKeycode.restype = ctypes.c_ubyte
    x11.XFlush.argtypes = [ctypes.c_void_p]
    x11.XFlush.restype = ctypes.c_int
    x11.XConnectionNumber.argtypes = [ctypes.c_void_p]
    x11.XConnectionNumber.restype = ctypes.c_int

if xtst:
    xtst.XTestFakeKeyEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
    xtst.XTestFakeKeyEvent.restype = ctypes.c_int

X11_KEYS_MAP = {
    "Shift_L": "Key.shift_l",
    "Shift_R": "Key.shift_r",
    "Control_L": "Key.ctrl_l",
    "Control_R": "Key.ctrl_r",
    "Alt_L": "Key.alt_l",
    "Alt_R": "Key.alt_r",
    "Super_L": "Key.cmd_l",
    "Super_R": "Key.cmd_r",
    "BackSpace": "Key.backspace",
    "Tab": "Key.tab",
    "Return": "Key.enter",
    "Escape": "Key.esc",
    "space": "Key.space",
    "Delete": "Key.delete",
    "Home": "Key.home",
    "End": "Key.end",
    "Prior": "Key.page_up",
    "Next": "Key.page_down",
    "Left": "Key.left",
    "Right": "Key.right",
    "Up": "Key.up",
    "Down": "Key.down",
    "Insert": "Key.insert",
    "Menu": "Key.menu",
    "Num_Lock": "Key.num_lock",
    "Print": "Key.print_screen",
    "Scroll_Lock": "Key.scroll_lock",
    "Caps_Lock": "Key.caps_lock",
    "Pause": "Key.pause",
}

for i in range(1, 21):
    X11_KEYS_MAP[f"F{i}"] = f"Key.f{i}"

X11_REVERSE_KEYS_MAP = {v: k for k, v in X11_KEYS_MAP.items()}
X11_REVERSE_KEYS_MAP["Key.cmd"] = "Super_L"
X11_REVERSE_KEYS_MAP["Key.shift"] = "Shift_L"
X11_REVERSE_KEYS_MAP["Key.ctrl"] = "Control_L"
X11_REVERSE_KEYS_MAP["Key.alt"] = "Alt_L"

X11_SYMBOL_MAP = {
    "-": "minus",
    "=": "equal",
    "[": "bracketleft",
    "]": "bracketright",
    ";": "semicolon",
    "'": "apostrophe",
    "\\": "backslash",
    ",": "comma",
    ".": "period",
    "/": "slash",
    "`": "grave",
    " ": "space",
    "!": "exclam",
    "@": "at",
    "#": "numbersign",
    "$": "dollar",
    "%": "percent",
    "^": "asciicircum",
    "&": "ampersand",
    "*": "asterisk",
    "(": "parenleft",
    ")": "parenright",
    "_": "underscore",
    "+": "plus",
    "{": "braceleft",
    "}": "braceright",
    "|": "bar",
    ":": "colon",
    "\"": "quotedbl",
    "<": "less",
    ">": "greater",
    "?": "question",
    "~": "asciitilde",
    "§": "section"
}

def x11_keycode_from_key(display, key):
    if not x11:
        return 0
        
    key = str(key)
    # Strip quotes if present (e.g. "'a'" -> 'a')
    if len(key) >= 3 and key[0] == "'" and key[-1] == "'":
        key = key[1:-1]
        
    if key.startswith("Key."):
        keysym_name = X11_REVERSE_KEYS_MAP.get(key)
    else:
        keysym_name = X11_SYMBOL_MAP.get(key, key)
    
    if not keysym_name:
        return 0
        
    keysym = x11.XStringToKeysym(keysym_name.encode('utf-8'))
    if keysym == 0:
        return 0
    return x11.XKeysymToKeycode(display, keysym)



class LinuxKeyboardController(BaseKeyboardController):
    def __init__(self):
        if x11:
            self.display = x11.XOpenDisplay(None)
        else:
            self.display = None

    def __del__(self):
        if x11 and self.display:
            x11.XCloseDisplay(self.display)

    def press(self, key):
        if not xtst or not self.display:
            return
        keycode = x11_keycode_from_key(self.display, key)
        if keycode != 0:
            xtst.XTestFakeKeyEvent(self.display, keycode, True, 0)
            x11.XFlush(self.display)
        else:
            logger.warning("LinuxKeyboardController: Key not mapped: %s", key)

    def release(self, key):
        if not xtst or not self.display:
            return
        keycode = x11_keycode_from_key(self.display, key)
        if keycode != 0:
            xtst.XTestFakeKeyEvent(self.display, keycode, False, 0)
            x11.XFlush(self.display)
        else:
            logger.warning("LinuxKeyboardController: Key not mapped: %s", key)


class LinuxKeyboardListener(BaseKeyboardListener, threading.Thread):
    def __init__(self, on_press=None, on_release=None, suppress=False):
        threading.Thread.__init__(self)
        self.on_press = on_press
        self.on_release = on_release
        self.suppress = suppress
        self.display = None
        self._stopping = False
        self.daemon = True

    def run(self):
        if not x11:
            logger.error("X11 library not available")
            return
        self.display = x11.XOpenDisplay(None)
        if not self.display:
            logger.error("Failed to open X11 display")
            return
            
        root = x11.XDefaultRootWindow(self.display)
        
        try:
            if self.suppress:
                res = x11.XGrabKeyboard(self.display, root, True, 1, 1, 0)
                if res != 0:
                    logger.error("XGrabKeyboard failed: %d", res)
                    
            import select
            fd = x11.XConnectionNumber(self.display)
            event = XEvent()
            
            while not self._stopping:
                # Block at OS level until data arrives (0.5s timeout)
                r, _, _ = select.select([fd], [], [], 0.5)
                if r:
                    while x11.XPending(self.display) > 0:
                        x11.XNextEvent(self.display, ctypes.byref(event))
                        
                        if event.type in (2, 3):
                            keycode = event.xkey.keycode
                            keysym = x11.XKeycodeToKeysym(self.display, keycode, 0)
                            sym_bytes = x11.XKeysymToString(keysym)
                            if sym_bytes:
                                sym_name = sym_bytes.decode('utf-8', errors='ignore')
                                key_str = X11_KEYS_MAP.get(sym_name)
                                if not key_str:
                                    key_str = f"'{sym_name}'" if len(sym_name) == 1 else f"'{sym_name}'"
                                    
                                if event.type == 2:
                                    if self.on_press:
                                        self.on_press(key_str)
                                else:
                                    if self.on_release:
                                        self.on_release(key_str)
        finally:
            if self.suppress:
                x11.XUngrabKeyboard(self.display, 0)
            x11.XCloseDisplay(self.display)
            self.display = None

    def stop(self):
        self._stopping = True

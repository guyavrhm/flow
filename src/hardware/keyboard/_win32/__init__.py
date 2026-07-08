import ctypes
from ctypes import wintypes
import logging
import threading

from src.hardware.keyboard._base import BaseKeyboardController, BaseKeyboardListener

logger = logging.getLogger(__name__)

# Load Windows libraries
try:
    user32 = ctypes.windll.user32
    kernel32 = ctypes.windll.kernel32
except Exception:
    user32 = None
    kernel32 = None

WH_KEYBOARD_LL = 13
WM_KEYDOWN = 0x0100
WM_KEYUP = 0x0101
WM_SYSKEYDOWN = 0x0104
WM_SYSKEYUP = 0x0105

class KBDLLHOOKSTRUCT(ctypes.Structure):
    _fields_ = [
        ("vkCode", wintypes.DWORD),
        ("scanCode", wintypes.DWORD),
        ("flags", wintypes.DWORD),
        ("time", wintypes.DWORD),
        ("dwExtraInfo", ctypes.POINTER(ctypes.c_ulong)),
    ]

class KEYBDINPUT(ctypes.Structure):
    _fields_ = [
        ("wVk", wintypes.WORD),
        ("wScan", wintypes.WORD),
        ("dwFlags", wintypes.DWORD),
        ("time", wintypes.DWORD),
        ("dwExtraInfo", ctypes.POINTER(ctypes.c_ulong)),
    ]

class INPUT_UNION(ctypes.Union):
    _fields_ = [
        ("ki", KEYBDINPUT),
        ("pad", ctypes.c_byte * 40)
    ]

class INPUT(ctypes.Structure):
    _fields_ = [
        ("type", wintypes.DWORD),
        ("u", INPUT_UNION),
    ]

HOOKPROC = ctypes.WINFUNCTYPE(ctypes.c_longlong, ctypes.c_int, wintypes.WPARAM, wintypes.LPARAM)

WIN32_KEYS = {
    0x08: "Key.backspace",
    0x09: "Key.tab",
    0x0D: "Key.enter",
    0x10: "Key.shift",
    0x11: "Key.ctrl",
    0x12: "Key.alt",
    0x14: "Key.caps_lock",
    0x1B: "Key.esc",
    0x20: "Key.space",
    0x21: "Key.page_up",
    0x22: "Key.page_down",
    0x23: "Key.end",
    0x24: "Key.home",
    0x25: "Key.left",
    0x26: "Key.up",
    0x27: "Key.right",
    0x28: "Key.down",
    0x2D: "Key.insert",
    0x2E: "Key.delete",
    0x5B: "Key.cmd_l",
    0x5C: "Key.cmd_r",
    0x5F: "Key.menu",
    0x90: "Key.num_lock",
    0x91: "Key.scroll_lock",
    0xA0: "Key.shift_l",
    0xA1: "Key.shift_r",
    0xA2: "Key.ctrl_l",
    0xA3: "Key.ctrl_r",
    0xA4: "Key.alt_l",
    0xA5: "Key.alt_r",
}

for i in range(1, 25):
    WIN32_KEYS[0x6F + i] = f"Key.f{i}"

WIN32_CHAR_MAP = {}
for vk in range(0x41, 0x5B):
    WIN32_CHAR_MAP[vk] = chr(vk).lower()
for vk in range(0x30, 0x3A):
    WIN32_CHAR_MAP[vk] = chr(vk)

WIN32_REVERSE_KEYS = {v: k for k, v in WIN32_KEYS.items()}
WIN32_REVERSE_KEYS["Key.shift"] = 0x10
WIN32_REVERSE_KEYS["Key.ctrl"] = 0x11
WIN32_REVERSE_KEYS["Key.alt"] = 0x12
WIN32_REVERSE_KEYS["Key.cmd"] = 0x5B

WIN32_SYMBOLS = {
    ';': 0xBA, '=': 0xBB, ',': 0xBC, '-': 0xBD, '.': 0xBE, '/': 0xBF, '`': 0xC0,
    '[': 0xDB, '\\': 0xDC, ']': 0xDD, "'": 0xDE
}
for k, v in WIN32_SYMBOLS.items():
    WIN32_REVERSE_KEYS[k] = v

def get_win32_vk(key):
    if key.startswith("Key."):
        return WIN32_REVERSE_KEYS.get(key)
    
    # Strip quotes if present (e.g. "'a'" -> 'a')
    if len(key) >= 3 and key[0] == "'" and key[-1] == "'":
        key = key[1:-1]
        
    char = key.lower()
    if len(char) == 1:
        if 'a' <= char <= 'z':
            return 0x41 + (ord(char) - ord('a'))
        if '0' <= char <= '9':
            return 0x30 + (ord(char) - ord('0'))
        return WIN32_SYMBOLS.get(char)
    return None



class WindowsKeyboardController(BaseKeyboardController):
    def press(self, key):
        if not user32:
            return
        vk = get_win32_vk(key)
        if vk:
            inp = INPUT()
            inp.type = 1
            inp.u.ki.wVk = vk
            inp.u.ki.dwFlags = 0
            user32.SendInput(1, ctypes.byref(inp), ctypes.sizeof(INPUT))
        else:
            logger.warning("WindowsKeyboardController: Key not mapped: %s", key)

    def release(self, key):
        if not user32:
            return
        vk = get_win32_vk(key)
        if vk:
            inp = INPUT()
            inp.type = 1
            inp.u.ki.wVk = vk
            inp.u.ki.dwFlags = 2
            user32.SendInput(1, ctypes.byref(inp), ctypes.sizeof(INPUT))
        else:
            logger.warning("WindowsKeyboardController: Key not mapped: %s", key)


class WindowsKeyboardListener(BaseKeyboardListener, threading.Thread):
    def __init__(self, on_press=None, on_release=None, suppress=False):
        threading.Thread.__init__(self)
        self.on_press = on_press
        self.on_release = on_release
        self.suppress = suppress
        self.hook = None
        self.thread_id = None
        self.daemon = True
        self._stopping = False

    def run(self):
        if not user32 or not kernel32:
            return
            
        self.thread_id = kernel32.GetCurrentThreadId()
        if self._stopping:
            return
        
        def hook_proc(nCode, wParam, lParam):
            if nCode >= 0:
                try:
                    kbd = KBDLLHOOKSTRUCT.from_address(lParam)
                    vk = kbd.vkCode
                    key_str = WIN32_KEYS.get(vk)
                    if not key_str:
                        char = WIN32_CHAR_MAP.get(vk)
                        if not char:
                            char_code = user32.MapVirtualKeyW(vk, 2)
                            char = chr(char_code) if char_code else None
                        key_str = f"'{char}'" if char else f"'{chr(vk)}'"
                    
                    pressed = wParam in (WM_KEYDOWN, WM_SYSKEYDOWN)
                    if pressed:
                        if self.on_press:
                            self.on_press(key_str)
                    else:
                        if self.on_release:
                            self.on_release(key_str)
                            
                    if self.suppress:
                        return 1
                except Exception as e:
                    logger.error("Error in Windows low-level keyboard hook callback: %s", e)
            return user32.CallNextHookEx(self.hook, nCode, wParam, lParam)

        self._hook_callback = HOOKPROC(hook_proc)
        h_mod = kernel32.GetModuleHandleW(None)
        
        if self._stopping:
            return
            
        self.hook = user32.SetWindowsHookExW(WH_KEYBOARD_LL, self._hook_callback, h_mod, 0)
        if not self.hook:
            logger.critical("Failed to install low-level keyboard hook")
            return
            
        msg = wintypes.MSG()
        while user32.GetMessageW(ctypes.byref(msg), 0, 0, 0) != 0:
            user32.TranslateMessage(ctypes.byref(msg))
            user32.DispatchMessageW(ctypes.byref(msg))

    def stop(self):
        self._stopping = True
        if user32 and self.hook:
            user32.UnhookWindowsHookEx(self.hook)
            self.hook = None
        if user32 and self.thread_id:
            user32.PostThreadMessageW(self.thread_id, 0x0012, 0, 0)
            self.thread_id = None

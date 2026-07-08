import ctypes
from ctypes import wintypes
import logging
import threading
import time

from src.hardware.info import get_screeninfo
from src.hardware.mouse._base import BaseMouseController, BaseMouseListener

logger = logging.getLogger(__name__)

# Load Windows libraries
try:
    user32 = ctypes.windll.user32
    kernel32 = ctypes.windll.kernel32
except Exception:
    user32 = None
    kernel32 = None

WH_MOUSE_LL = 14
WM_MOUSEMOVE = 0x0200
WM_LBUTTONDOWN = 0x0201
WM_LBUTTONUP = 0x0202
WM_RBUTTONDOWN = 0x0204
WM_RBUTTONUP = 0x0205
WM_MBUTTONDOWN = 0x0207
WM_MBUTTONUP = 0x0208
WM_MOUSEWHEEL = 0x020A
WM_MOUSEHWHEEL = 0x020E
WM_XBUTTONDOWN = 0x020B
WM_XBUTTONUP = 0x020C

class POINT(ctypes.Structure):
    _fields_ = [
        ("x", wintypes.LONG),
        ("y", wintypes.LONG),
    ]

class MSLLHOOKSTRUCT(ctypes.Structure):
    _fields_ = [
        ("pt", POINT),
        ("mouseData", wintypes.DWORD),
        ("flags", wintypes.DWORD),
        ("time", wintypes.DWORD),
        ("dwExtraInfo", ctypes.POINTER(ctypes.c_ulong)),
    ]

class MOUSEINPUT(ctypes.Structure):
    _fields_ = [
        ("dx", wintypes.LONG),
        ("dy", wintypes.LONG),
        ("mouseData", wintypes.DWORD),
        ("dwFlags", wintypes.DWORD),
        ("time", wintypes.DWORD),
        ("dwExtraInfo", ctypes.POINTER(ctypes.c_ulong)),
    ]

class INPUT_UNION(ctypes.Union):
    _fields_ = [
        ("mi", MOUSEINPUT),
        ("pad", ctypes.c_byte * 40)
    ]

class INPUT(ctypes.Structure):
    _fields_ = [
        ("type", wintypes.DWORD),
        ("u", INPUT_UNION),
    ]

HOOKPROC = ctypes.WINFUNCTYPE(ctypes.c_longlong, ctypes.c_int, wintypes.WPARAM, wintypes.LPARAM)

WIN32_BUTTONS = {
    "left": (0x0002, 0x0004, 0),
    "right": (0x0008, 0x0010, 0),
    "middle": (0x0020, 0x0040, 0),
    "x1": (0x0080, 0x0100, 1),
    "x2": (0x0080, 0x0100, 2),
    "button8": (0x0080, 0x0100, 1),
    "button9": (0x0080, 0x0100, 2),
}

def set_global_cursor_hidden(hidden):
    if not user32 or not kernel32:
        return
    if hidden:
        cursor_ids = [32512, 32513, 32514, 32515, 32516, 32649]
        for cid in cursor_ids:
            and_mask = ctypes.c_byte(255)
            xor_mask = ctypes.c_byte(0)
            h_cursor = user32.CreateCursor(kernel32.GetModuleHandleW(None), 0, 0, 1, 1, ctypes.byref(and_mask), ctypes.byref(xor_mask))
            user32.SetSystemCursor(h_cursor, cid)
    else:
        user32.SystemParametersInfoW(0x0057, 0, None, 0)


class WindowsMouseController(BaseMouseController):
    @property
    def position(self):
        if not user32:
            return (0, 0)
        pt = POINT()
        user32.GetCursorPos(ctypes.byref(pt))
        return (pt.x, pt.y)

    @position.setter
    def position(self, pos):
        if not user32:
            return
        x, y = pos
        user32.SetCursorPos(int(x), int(y))

    def press(self, button):
        if not user32:
            return
        btn_info = WIN32_BUTTONS.get(button.lower())
        if btn_info:
            down_flag, _, data = btn_info
            inp = INPUT()
            inp.type = 0
            inp.u.mi.dwFlags = down_flag
            inp.u.mi.mouseData = data
            user32.SendInput(1, ctypes.byref(inp), ctypes.sizeof(INPUT))
        else:
            logger.warning("WindowsMouseController: Unknown press button %s", button)

    def release(self, button):
        if not user32:
            return
        btn_info = WIN32_BUTTONS.get(button.lower())
        if btn_info:
            _, up_flag, data = btn_info
            inp = INPUT()
            inp.type = 0
            inp.u.mi.dwFlags = up_flag
            inp.u.mi.mouseData = data
            user32.SendInput(1, ctypes.byref(inp), ctypes.sizeof(INPUT))
        else:
            logger.warning("WindowsMouseController: Unknown release button %s", button)

    def scroll(self, dx, dy):
        if not user32:
            return
        if dy != 0:
            inp = INPUT()
            inp.type = 0
            inp.u.mi.dwFlags = 0x0800
            inp.u.mi.mouseData = int(dy * 120)
            user32.SendInput(1, ctypes.byref(inp), ctypes.sizeof(INPUT))
        if dx != 0:
            inp = INPUT()
            inp.type = 0
            inp.u.mi.dwFlags = 0x01000
            inp.u.mi.mouseData = int(dx * 120)
            user32.SendInput(1, ctypes.byref(inp), ctypes.sizeof(INPUT))


class WindowsMouseListener(BaseMouseListener, threading.Thread):
    def __init__(self, on_move=None, on_click=None, on_scroll=None, suppress=False):
        threading.Thread.__init__(self)
        self.on_move = on_move
        self.on_click = on_click
        self.on_scroll = on_scroll
        self.suppress = suppress
        self.hook = None
        self.thread_id = None
        self.daemon = True
        self._stopping = False

        if self.on_move:
            self.metrics = get_screeninfo()
            self.x_center = int(self.metrics[0] / 2)
            self.y_center = int(self.metrics[1] / 2)

    def run(self):
        if not user32 or not kernel32:
            return
            
        self.thread_id = kernel32.GetCurrentThreadId()
        if self._stopping:
            return
        
        if self.on_move:
            user32.SetCursorPos(self.x_center, self.y_center)
            set_global_cursor_hidden(True)

        def hook_proc(nCode, wParam, lParam):
            if nCode >= 0:
                try:
                    ms = MSLLHOOKSTRUCT.from_address(lParam)
                    x, y = ms.pt.x, ms.pt.y
                    
                    # 1. Handle Movement
                    if self.on_move and wParam == WM_MOUSEMOVE:
                        if int(x) == self.x_center and int(y) == self.y_center:
                            return 1
                            
                        dx = x - self.x_center
                        dy = y - self.y_center
                        
                        if dx != 0 or dy != 0:
                            user32.SetCursorPos(self.x_center, self.y_center)
                            self.on_move(dx, dy)
                        return 1
                    
                    # 2. Handle Clicks
                    elif wParam in (WM_LBUTTONDOWN, WM_LBUTTONUP,
                                    WM_RBUTTONDOWN, WM_RBUTTONUP,
                                    WM_MBUTTONDOWN, WM_MBUTTONUP,
                                    WM_XBUTTONDOWN, WM_XBUTTONUP):
                        
                        pressed = wParam in (WM_LBUTTONDOWN, WM_RBUTTONDOWN, WM_MBUTTONDOWN, WM_XBUTTONDOWN)
                        btn = None
                        if wParam in (WM_LBUTTONDOWN, WM_LBUTTONUP):
                            btn = "Button.left"
                        elif wParam in (WM_RBUTTONDOWN, WM_RBUTTONUP):
                            btn = "Button.right"
                        elif wParam in (WM_MBUTTONDOWN, WM_MBUTTONUP):
                            btn = "Button.middle"
                        elif wParam in (WM_XBUTTONDOWN, WM_XBUTTONUP):
                            x_btn = (ms.mouseData >> 16) & 0xFFFF
                            btn = "Button.x1" if x_btn == 1 else "Button.x2"
                        
                        if self.on_click and btn:
                            self.on_click(x, y, btn, pressed)

                    # 3. Handle Scroll
                    elif wParam == WM_MOUSEWHEEL:
                        wheel_delta = ctypes.c_short((ms.mouseData >> 16) & 0xFFFF).value
                        dy = wheel_delta / 120.0
                        if self.on_scroll:
                            self.on_scroll(x, y, 0, dy)

                    elif wParam == WM_MOUSEHWHEEL:
                        wheel_delta = ctypes.c_short((ms.mouseData >> 16) & 0xFFFF).value
                        dx = wheel_delta / 120.0
                        if self.on_scroll:
                            self.on_scroll(x, y, dx, 0)
                            
                    if self.suppress:
                        return 1
                except Exception as e:
                    logger.error("Error in Windows low-level mouse hook callback: %s", e)
            return user32.CallNextHookEx(self.hook, nCode, wParam, lParam)

        self._hook_callback = HOOKPROC(hook_proc)
        h_mod = kernel32.GetModuleHandleW(None)
        
        if self._stopping:
            return
            
        self.hook = user32.SetWindowsHookExW(WH_MOUSE_LL, self._hook_callback, h_mod, 0)
        if not self.hook:
            logger.critical("Failed to install low-level mouse hook")
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
        if self.on_move:
            set_global_cursor_hidden(False)
        if user32 and self.thread_id:
            user32.PostThreadMessageW(self.thread_id, 0x0012, 0, 0)
            self.thread_id = None

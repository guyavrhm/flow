import ctypes
import ctypes.util
import logging
import threading
import time

from src.hardware.info import get_screeninfo
from src.hardware.mouse._base import BaseMouseController, BaseMouseListener

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

class XColor(ctypes.Structure):
    _fields_ = [
        ("pixel", ctypes.c_ulong),
        ("red", ctypes.c_ushort),
        ("green", ctypes.c_ushort),
        ("blue", ctypes.c_ushort),
        ("flags", ctypes.c_char),
        ("pad", ctypes.c_char),
    ]

class XAnyEvent(ctypes.Structure):
    _fields_ = [
        ("type", ctypes.c_int),
        ("serial", ctypes.c_ulong),
        ("send_event", ctypes.c_int),
        ("display", ctypes.c_void_p),
        ("window", ctypes.c_ulong),
    ]

class XButtonEvent(ctypes.Structure):
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
        ("button", ctypes.c_uint),
        ("same_screen", ctypes.c_int),
    ]

class XMotionEvent(ctypes.Structure):
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
        ("is_hint", ctypes.c_char),
        ("same_screen", ctypes.c_int),
    ]

class XEvent(ctypes.Union):
    _fields_ = [
        ("type", ctypes.c_int),
        ("xany", XAnyEvent),
        ("xbutton", XButtonEvent),
        ("xmotion", XMotionEvent),
        ("pad", ctypes.c_char * 192),
    ]

if x11:
    x11.XOpenDisplay.argtypes = [ctypes.c_char_p]
    x11.XOpenDisplay.restype = ctypes.c_void_p
    x11.XCloseDisplay.argtypes = [ctypes.c_void_p]
    x11.XCloseDisplay.restype = ctypes.c_int
    x11.XDefaultRootWindow.argtypes = [ctypes.c_void_p]
    x11.XDefaultRootWindow.restype = ctypes.c_ulong
    x11.XQueryPointer.argtypes = [
        ctypes.c_void_p, ctypes.c_ulong,
        ctypes.POINTER(ctypes.c_ulong), ctypes.POINTER(ctypes.c_ulong),
        ctypes.POINTER(ctypes.c_int), ctypes.POINTER(ctypes.c_int),
        ctypes.POINTER(ctypes.c_int), ctypes.POINTER(ctypes.c_int),
        ctypes.POINTER(ctypes.c_uint)
    ]
    x11.XQueryPointer.restype = ctypes.c_int
    x11.XWarpPointer.argtypes = [
        ctypes.c_void_p, ctypes.c_ulong, ctypes.c_ulong,
        ctypes.c_int, ctypes.c_int, ctypes.c_uint, ctypes.c_uint,
        ctypes.c_int, ctypes.c_int
    ]
    x11.XWarpPointer.restype = ctypes.c_int
    x11.XGrabPointer.argtypes = [
        ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int, ctypes.c_uint,
        ctypes.c_int, ctypes.c_int, ctypes.c_ulong, ctypes.c_ulong, ctypes.c_ulong
    ]
    x11.XGrabPointer.restype = ctypes.c_int
    x11.XUngrabPointer.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
    x11.XUngrabPointer.restype = ctypes.c_int
    x11.XCreateBitmapFromData.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_char_p, ctypes.c_uint, ctypes.c_uint]
    x11.XCreateBitmapFromData.restype = ctypes.c_ulong
    x11.XCreatePixmapCursor.argtypes = [
        ctypes.c_void_p, ctypes.c_ulong, ctypes.c_ulong,
        ctypes.POINTER(XColor), ctypes.POINTER(XColor), ctypes.c_uint, ctypes.c_uint
    ]
    x11.XCreatePixmapCursor.restype = ctypes.c_ulong
    x11.XFreePixmap.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
    x11.XFreePixmap.restype = ctypes.c_int
    x11.XFreeCursor.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
    x11.XFreeCursor.restype = ctypes.c_int
    x11.XPending.argtypes = [ctypes.c_void_p]
    x11.XPending.restype = ctypes.c_int
    x11.XNextEvent.argtypes = [ctypes.c_void_p, ctypes.POINTER(XEvent)]
    x11.XNextEvent.restype = ctypes.c_int
    x11.XFlush.argtypes = [ctypes.c_void_p]
    x11.XFlush.restype = ctypes.c_int
    x11.XConnectionNumber.argtypes = [ctypes.c_void_p]
    x11.XConnectionNumber.restype = ctypes.c_int

if xtst:
    xtst.XTestFakeButtonEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
    xtst.XTestFakeButtonEvent.restype = ctypes.c_int
    xtst.XTestFakeMotionEvent.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_ulong]
    xtst.XTestFakeMotionEvent.restype = ctypes.c_int

X11_BUTTONS = {
    "left": 1,
    "middle": 2,
    "right": 3,
    "button8": 8,
    "button9": 9,
    "x1": 8,
    "x2": 9
}

class LinuxMouseController(BaseMouseController):
    def __init__(self):
        if x11:
            self.display = x11.XOpenDisplay(None)
        else:
            self.display = None

    def __del__(self):
        if x11 and self.display:
            x11.XCloseDisplay(self.display)

    @property
    def position(self):
        if not x11 or not self.display:
            return (0, 0)
        root = x11.XDefaultRootWindow(self.display)
        root_w = ctypes.c_ulong()
        child_w = ctypes.c_ulong()
        root_x = ctypes.c_int()
        root_y = ctypes.c_int()
        win_x = ctypes.c_int()
        win_y = ctypes.c_int()
        mask = ctypes.c_uint()
        x11.XQueryPointer(
            self.display, root,
            ctypes.byref(root_w), ctypes.byref(child_w),
            ctypes.byref(root_x), ctypes.byref(root_y),
            ctypes.byref(win_x), ctypes.byref(win_y),
            ctypes.byref(mask)
        )
        return (root_x.value, root_y.value)

    @position.setter
    def position(self, pos):
        if not xtst or not self.display:
            return
        x, y = pos
        xtst.XTestFakeMotionEvent(self.display, -1, int(x), int(y), 0)
        x11.XFlush(self.display)

    def press(self, button):
        if not xtst or not self.display:
            return
        btn_num = X11_BUTTONS.get(button.lower(), 1)
        xtst.XTestFakeButtonEvent(self.display, btn_num, True, 0)
        x11.XFlush(self.display)

    def release(self, button):
        if not xtst or not self.display:
            return
        btn_num = X11_BUTTONS.get(button.lower(), 1)
        xtst.XTestFakeButtonEvent(self.display, btn_num, False, 0)
        x11.XFlush(self.display)

    def scroll(self, dx, dy):
        if not xtst or not self.display:
            return
        if dy != 0:
            btn = 4 if dy > 0 else 5
            for _ in range(abs(dy)):
                xtst.XTestFakeButtonEvent(self.display, btn, True, 0)
                xtst.XTestFakeButtonEvent(self.display, btn, False, 0)
        if dx != 0:
            btn = 7 if dx > 0 else 6
            for _ in range(abs(dx)):
                xtst.XTestFakeButtonEvent(self.display, btn, True, 0)
                xtst.XTestFakeButtonEvent(self.display, btn, False, 0)
        x11.XFlush(self.display)


class LinuxMouseListener(BaseMouseListener, threading.Thread):
    def __init__(self, on_move=None, on_click=None, on_scroll=None, suppress=False):
        threading.Thread.__init__(self)
        self.on_move = on_move
        self.on_click = on_click
        self.on_scroll = on_scroll
        self.suppress = suppress
        self.display = None
        self._stopping = False
        self.cursor = 0
        self.bitmap = 0
        self.daemon = True

        if self.on_move:
            self.metrics = get_screeninfo()
            self.x_center = int(self.metrics[0] / 2)
            self.y_center = int(self.metrics[1] / 2)

    def run(self):
        if not x11:
            return
        self.display = x11.XOpenDisplay(None)
        if not self.display:
            return
        root = x11.XDefaultRootWindow(self.display)
        
        try:
            if self.on_move:
                x11.XWarpPointer(self.display, 0, root, 0, 0, 0, 0, self.x_center, self.y_center)
                data = (ctypes.c_char * 8)(0, 0, 0, 0, 0, 0, 0, 0)
                self.bitmap = x11.XCreateBitmapFromData(self.display, root, data, 8, 8)
                black = XColor(0, 0, 0, 0, 0, 0)
                self.cursor = x11.XCreatePixmapCursor(self.display, self.bitmap, self.bitmap, ctypes.byref(black), ctypes.byref(black), 0, 0)
            
            event_mask = 4 | 8
            if self.on_move or self.suppress:
                event_mask |= 64
                
            x11.XGrabPointer(self.display, root, True, event_mask, 1, 1, 0, self.cursor, 0)
            x11.XFlush(self.display)
                
            import select
            fd = x11.XConnectionNumber(self.display)
            event = XEvent()
            
            while not self._stopping:
                # Block at OS level until data arrives (0.5s timeout)
                r, _, _ = select.select([fd], [], [], 0.5)
                if r:
                    while x11.XPending(self.display) > 0:
                        x11.XNextEvent(self.display, ctypes.byref(event))
                        
                        # 1. Handle Movement
                        if self.on_move and event.type == 6:
                            x, y = event.xmotion.x_root, event.xmotion.y_root
                            
                            if int(x) == self.x_center and int(y) == self.y_center:
                                continue
                                
                            dx = x - self.x_center
                            dy = y - self.y_center
                            
                            if dx != 0 or dy != 0:
                                x11.XWarpPointer(self.display, 0, root, 0, 0, 0, 0, self.x_center, self.y_center)
                                x11.XFlush(self.display)
                                self.on_move(dx, dy)

                        # 2. Handle Clicks & Scroll
                        elif event.type in (4, 5):
                            btn = event.xbutton.button
                            x, y = event.xbutton.x_root, event.xbutton.y_root
                            
                            if btn in (4, 5, 6, 7):
                                if event.type == 4:
                                    dy = 1 if btn == 4 else -1 if btn == 5 else 0
                                    dx = 1 if btn == 7 else -1 if btn == 6 else 0
                                    if self.on_scroll:
                                        self.on_scroll(x, y, dx, dy)
                            else:
                                btn_name = "Button.left"
                                if btn == 2:
                                    btn_name = "Button.middle"
                                elif btn == 3:
                                    btn_name = "Button.right"
                                elif btn == 8:
                                    btn_name = "Button.x1"
                                elif btn == 9:
                                    btn_name = "Button.x2"
                                else:
                                    btn_name = f"Button.button{btn}"
                                    
                                if self.on_click:
                                    self.on_click(x, y, btn_name, event.type == 4)
        finally:
            x11.XUngrabPointer(self.display, 0)
            if self.cursor:
                x11.XFreeCursor(self.display, self.cursor)
            if self.bitmap:
                x11.XFreePixmap(self.display, self.bitmap)
            x11.XCloseDisplay(self.display)
            self.display = None

    def stop(self):
        self._stopping = True

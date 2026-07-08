import ctypes
import ctypes.util
import logging
import threading
from AppKit import NSEvent
from Quartz import (
    CGEventTapCreate,
    kCGSessionEventTap,
    kCGHeadInsertEventTap,
    kCGEventTapOptionDefault,
    CGEventMaskBit,
    kCGEventKeyDown,
    kCGEventKeyUp,
    kCGEventFlagsChanged,
    CFRunLoopGetCurrent,
    CFRunLoopAddSource,
    kCFRunLoopDefaultMode,
    CFRunLoopRun,
    CFRunLoopStop,
    CGEventTapEnable,
    CGEventGetFlags,
    CFMachPortCreateRunLoopSource,
    CGEventGetIntegerValueField,
    kCGKeyboardEventKeycode,
    CGEventSourceKeyState,
    kCGEventSourceStateCombinedSessionState,
    CGEventCreateKeyboardEvent,
    CGEventPost,
    kCGHIDEventTap
)

from src.hardware.keyboard._base import BaseKeyboardController, BaseKeyboardListener

logger = logging.getLogger(__name__)

# Load Carbon library for keycode translation
carbon_path = ctypes.util.find_library('Carbon')
if not carbon_path:
    carbon_path = '/System/Library/Frameworks/Carbon.framework/Carbon'
try:
    carbon = ctypes.CDLL(carbon_path)
    carbon.TISCopyCurrentKeyboardInputSource.argtypes = []
    carbon.TISCopyCurrentKeyboardInputSource.restype = ctypes.c_void_p
    carbon.TISGetInputSourceProperty.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
    carbon.TISGetInputSourceProperty.restype = ctypes.c_void_p
    kTISPropertyUnicodeKeyLayoutData = ctypes.c_void_p.in_dll(carbon, 'kTISPropertyUnicodeKeyLayoutData')
except Exception:
    carbon = None
    kTISPropertyUnicodeKeyLayoutData = None

def keycode_to_char_mac(keycode, modifier_state=0):
    if not carbon or kTISPropertyUnicodeKeyLayoutData is None:
        return None
    try:
        tis_source = carbon.TISCopyCurrentKeyboardInputSource()
        if not tis_source:
            return None
        
        layout_data_ptr = carbon.TISGetInputSourceProperty(tis_source, kTISPropertyUnicodeKeyLayoutData)
        if not layout_data_ptr:
            return None
        
        carbon.UCKeyTranslate.argtypes = [
            ctypes.c_void_p,
            ctypes.c_ushort,
            ctypes.c_ushort,
            ctypes.c_uint,
            ctypes.c_uint,
            ctypes.c_uint,
            ctypes.POINTER(ctypes.c_uint),
            ctypes.c_ulong,
            ctypes.POINTER(ctypes.c_ulong),
            ctypes.c_wchar_p
        ]
        carbon.UCKeyTranslate.restype = ctypes.c_int
        
        dead_keys = ctypes.c_uint(0)
        actual_len = ctypes.c_ulong(0)
        unicode_str = ctypes.create_unicode_buffer(10)
        
        status = carbon.UCKeyTranslate(
            layout_data_ptr,
            keycode,
            0,
            modifier_state,
            0,
            1,
            ctypes.byref(dead_keys),
            10,
            ctypes.byref(actual_len),
            unicode_str
        )
        
        if status == 0 and actual_len.value > 0:
            return unicode_str.value[:actual_len.value]
    except Exception as e:
        logger.debug("Failed to translate keycode %d to char: %s", keycode, e)
    return None

MACOS_KEY_MAP = {
    51: "Key.backspace",
    48: "Key.tab",
    36: "Key.enter",
    53: "Key.esc",
    49: "Key.space",
    115: "Key.home",
    119: "Key.end",
    116: "Key.page_up",
    121: "Key.page_down",
    117: "Key.delete",
    123: "Key.left",
    124: "Key.right",
    125: "Key.down",
    126: "Key.up",
    55: "Key.cmd_l",
    54: "Key.cmd_r",
    56: "Key.shift_l",
    60: "Key.shift_r",
    59: "Key.ctrl_l",
    62: "Key.ctrl_r",
    58: "Key.alt_l",
    61: "Key.alt_r",
    57: "Key.caps_lock",
    122: "Key.f1",
    120: "Key.f2",
    99: "Key.f3",
    118: "Key.f4",
    96: "Key.f5",
    97: "Key.f6",
    98: "Key.f7",
    100: "Key.f8",
    101: "Key.f9",
    109: "Key.f10",
    103: "Key.f11",
    111: "Key.f12",
}

MACOS_REVERSE_KEY_MAP = {v: k for k, v in MACOS_KEY_MAP.items()}
MACOS_REVERSE_KEY_MAP["Key.cmd"] = 55
MACOS_REVERSE_KEY_MAP["Key.shift"] = 56
MACOS_REVERSE_KEY_MAP["Key.ctrl"] = 59
MACOS_REVERSE_KEY_MAP["Key.alt"] = 58

_mac_char_to_keycode = {}
for code in range(128):
    char = keycode_to_char_mac(code)
    if char and len(char) == 1:
        _mac_char_to_keycode[char.lower()] = code

US_LAYOUT = {
    'a': 0, 'b': 11, 'c': 8, 'd': 2, 'e': 14, 'f': 3, 'g': 5, 'h': 4, 'i': 34,
    'j': 38, 'k': 40, 'l': 37, 'm': 46, 'n': 45, 'o': 31, 'p': 35, 'q': 12,
    'r': 15, 's': 1, 't': 17, 'u': 32, 'v': 9, 'w': 13, 'x': 7, 'y': 16, 'z': 6,
    '0': 29, '1': 18, '2': 19, '3': 20, '4': 21, '5': 23, '6': 22, '7': 26,
    '8': 28, '9': 25,
    ' ': 49, '\n': 36, '\r': 36, '\t': 48,
    '-': 27, '=': 24, '[': 33, ']': 30, '\\': 42, ';': 41, "'": 39, ',': 43,
    '.': 47, '/': 44, '`': 50
}

def get_mac_keycode(key):
    if key.startswith("Key."):
        return MACOS_REVERSE_KEY_MAP.get(key)
    
    # Strip quotes if present (e.g. "'a'" -> 'a')
    if len(key) >= 3 and key[0] == "'" and key[-1] == "'":
        key = key[1:-1]
        
    char = key.lower()
    if char in _mac_char_to_keycode:
        return _mac_char_to_keycode[char]
    return US_LAYOUT.get(char)



class MacOSKeyboardController(BaseKeyboardController):
    def press(self, key):
        keycode = get_mac_keycode(key)
        if keycode is not None:
            event = CGEventCreateKeyboardEvent(None, keycode, True)
            CGEventPost(kCGHIDEventTap, event)
        else:
            logger.warning("MacOSKeyboardController: Unknown key code for key %s", key)

    def release(self, key):
        keycode = get_mac_keycode(key)
        if keycode is not None:
            event = CGEventCreateKeyboardEvent(None, keycode, False)
            CGEventPost(kCGHIDEventTap, event)
        else:
            logger.warning("MacOSKeyboardController: Unknown key code for key %s", key)


class MacOSKeyboardListener(BaseKeyboardListener, threading.Thread):
    def __init__(self, on_press=None, on_release=None, suppress=False):
        threading.Thread.__init__(self)
        self.on_press = on_press
        self.on_release = on_release
        self.suppress = suppress
        self.runloop = None
        self.tap = None
        self.daemon = True

    def run(self):
        self.runloop = CFRunLoopGetCurrent()
        
        mask = (
            CGEventMaskBit(kCGEventKeyDown) |
            CGEventMaskBit(kCGEventKeyUp) |
            CGEventMaskBit(kCGEventFlagsChanged)
        )
        
        def callback(proxy, event_type, event, refcon):
            # Only process valid keyboard events; ignore timeout/disable tap events
            if event_type not in (kCGEventKeyDown, kCGEventKeyUp, kCGEventFlagsChanged):
                return event

            try:
                keycode = CGEventGetIntegerValueField(event, kCGKeyboardEventKeycode)
                key_str = MACOS_KEY_MAP.get(keycode)
                if not key_str:
                    ns_event = NSEvent.eventWithCGEvent_(event)
                    try:
                        chars = ns_event.charactersIgnoringModifiers()
                        if chars and len(chars) > 0:
                            key_str = f"'{chars[0]}'"
                    except Exception:
                        pass
                    
                    if not key_str:
                        char = keycode_to_char_mac(keycode)
                        key_str = f"'{char}'" if char else f"'{chr(keycode)}'"
                
                if event_type == kCGEventFlagsChanged:
                    flags = CGEventGetFlags(event)
                    # Extract flags state based on keycode
                    if keycode in (55, 54):  # Command keys
                        pressed = bool(flags & 0x00100000)
                    elif keycode in (56, 60):  # Shift keys
                        pressed = bool(flags & 0x00020000)
                    elif keycode in (59, 62):  # Control keys
                        pressed = bool(flags & 0x00040000)
                    elif keycode in (58, 61):  # Option/Alt keys
                        pressed = bool(flags & 0x00080000)
                    elif keycode == 57:  # Caps Lock
                        pressed = bool(flags & 0x00010000)
                    else:
                        pressed = False

                    if pressed:
                        if self.on_press:
                            self.on_press(key_str)
                    else:
                        if self.on_release:
                            self.on_release(key_str)
                elif event_type == kCGEventKeyDown:
                    if self.on_press:
                        self.on_press(key_str)
                elif event_type == kCGEventKeyUp:
                    if self.on_release:
                        self.on_release(key_str)
                
                if self.suppress:
                    return None
            except Exception as e:
                logger.error("Error in keyboard event tap: %s", e)
            return event

        self.tap = CGEventTapCreate(
            kCGSessionEventTap,
            kCGHeadInsertEventTap,
            kCGEventTapOptionDefault,
            mask,
            callback,
            None
        )
        if not self.tap:
            logger.critical("Failed to create keyboard event tap. Accessibility permission is required.")
            return

        source = CFMachPortCreateRunLoopSource(None, self.tap, 0)
        CFRunLoopAddSource(self.runloop, source, kCFRunLoopDefaultMode)
        CGEventTapEnable(self.tap, True)
        CFRunLoopRun()

    def stop(self):
        if self.tap:
            CGEventTapEnable(self.tap, False)
        if self.runloop:
            CFRunLoopStop(self.runloop)

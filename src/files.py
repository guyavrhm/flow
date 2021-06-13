"""
File location constants
"""
import os
import tempfile

import src.info.computerinfo as ci

os.chdir(os.path.dirname(os.path.realpath(__file__)))

if ci.platform == ci.WINDOWS:
    DATABASE = os.getenv('APPDATA') + '\\flow.db'
    AES_SO = 'network/aes/aes.dll'
else:
    DATABASE = 'flow.db'
    AES_SO = 'network/aes/aes.so'

TEMP_FLOW = tempfile.gettempdir() + "/flow"

WEB_PAGE = 'https://guyavrhm.github.io/flow'

# Resources
FLOW_PNG = 'resources/flow.png'
FLOWX_PNG = 'resources/flowx.png'
FLOWV_PNG = 'resources/flowv.png'

# Clipboard
FILE2CLIP_MAC = './hardware/clipboard/_darwin/file2clip.applescript'
GET_FILES_MAC = './hardware/clipboard/_darwin/getfiles.applescript'
FILE2CLIP_WIN = './hardware/clipboard/_win32/file2clip.exe'
XCOPY = './hardware/clipboard/_xorg/xcopy.sh'
XPASTE = './hardware/clipboard/_xorg/xpaste.sh'
MCOPY = './hardware/clipboard/_darwin/mcopy.sh'
MPASTE = './hardware/clipboard/_darwin/mpaste.sh'

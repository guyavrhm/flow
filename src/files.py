"""
File location constants
"""
import os
import tempfile

from src.hardware.info import get_app_dir, get_aes_extension


BASE_DIR = os.path.dirname(os.path.realpath(__file__))

FLOW_DIR = get_app_dir()
AES_SO = os.path.join(BASE_DIR, 'network', 'aes', f'aes.{get_aes_extension()}')

if not os.path.isdir(FLOW_DIR):
    try:
        os.makedirs(FLOW_DIR, exist_ok=True)
    except Exception:
        pass

DATABASE = os.path.join(FLOW_DIR, 'flow.db')
LOG_FILE = os.path.join(FLOW_DIR, 'flow.log')

TEMP_FLOW = os.path.join(tempfile.gettempdir(), "flow")
MAX_FILE_SIZE = 50 * 1024 * 1024  # 50 MB (temporary until streaming)

WEB_PAGE = 'https://guyavrhm.github.io/flow'

# Resources
FLOW_PNG = os.path.join(BASE_DIR, 'resources', 'flow.png')
FLOWX_PNG = os.path.join(BASE_DIR, 'resources', 'flowx.png')
FLOWV_PNG = os.path.join(BASE_DIR, 'resources', 'flowv.png')

# Clipboard scripts
FILE2CLIP_MAC = os.path.join(BASE_DIR, 'hardware', 'clipboard', '_darwin', 'file2clip.applescript')
GET_FILES_MAC = os.path.join(BASE_DIR, 'hardware', 'clipboard', '_darwin', 'getfiles.applescript')
FILE2CLIP_WIN = os.path.join(BASE_DIR, 'hardware', 'clipboard', '_win32', 'file2clip.exe')



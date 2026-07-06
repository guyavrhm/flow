"""
File location constants
"""
import sys
import os
import tempfile


BASE_DIR = os.path.dirname(os.path.realpath(__file__))

if sys.platform == 'win32':
    FLOW_DIR = os.path.join(os.getenv('APPDATA'), 'flow')
    AES_SO = os.path.join(BASE_DIR, 'network', 'aes', 'aes.dll')
else:
    FLOW_DIR = os.path.expanduser('~/.flow')
    AES_SO = os.path.join(BASE_DIR, 'network', 'aes', 'aes.so')

if not os.path.isdir(FLOW_DIR):
    try:
        os.makedirs(FLOW_DIR, exist_ok=True)
    except Exception:
        pass

DATABASE = os.path.join(FLOW_DIR, 'flow.db')
LOG_FILE = os.path.join(FLOW_DIR, 'flow.log')

TEMP_FLOW = os.path.join(tempfile.gettempdir(), "flow")

WEB_PAGE = 'https://guyavrhm.github.io/flow'

# Resources
FLOW_PNG = os.path.join(BASE_DIR, 'resources', 'flow.png')
FLOWX_PNG = os.path.join(BASE_DIR, 'resources', 'flowx.png')
FLOWV_PNG = os.path.join(BASE_DIR, 'resources', 'flowv.png')

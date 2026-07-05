"""
File location constants
"""
import sys
import os
import tempfile


BASE_DIR = os.path.dirname(os.path.realpath(__file__))

if sys.platform == 'win32':
    DATABASE = os.path.join(os.getenv('APPDATA'), 'flow.db')
    AES_SO = os.path.join(BASE_DIR, 'network', 'aes', 'aes.dll')
else:
    DATABASE = os.path.expanduser('~/.flow.db')
    AES_SO = os.path.join(BASE_DIR, 'network', 'aes', 'aes.so')

TEMP_FLOW = os.path.join(tempfile.gettempdir(), "flow")

WEB_PAGE = 'https://guyavrhm.github.io/flow'

# Resources
FLOW_PNG = os.path.join(BASE_DIR, 'resources', 'flow.png')
FLOWX_PNG = os.path.join(BASE_DIR, 'resources', 'flowx.png')
FLOWV_PNG = os.path.join(BASE_DIR, 'resources', 'flowv.png')

# -*- mode: python ; coding: utf-8 -*-

import sys
import os

block_cipher = None

# Resolve absolute path of the project root directory using PyInstaller's SPECPATH global
project_root = os.path.abspath(os.path.join(SPECPATH, '..')) if 'SPECPATH' in globals() else os.path.abspath(os.path.join(os.path.dirname(__file__), '..'))

# Platform-specific C shared library
aes_lib_name = 'aes.dll' if sys.platform == 'win32' else 'aes.so'
aes_lib_path = os.path.join(project_root, 'src', 'network', 'aes', aes_lib_name)

a = Analysis(
    [os.path.join(project_root, 'flow.py')],
    pathex=[project_root],
    binaries=[
        (aes_lib_path, 'src/network/aes')
    ],
    datas=[
        (os.path.join(project_root, 'src', 'resources', '*.png'), 'src/resources')
    ],
    hiddenimports=[
        # pynput backends are loaded dynamically and need to be explicitly listed
        'pynput.keyboard._darwin',
        'pynput.keyboard._win32',
        'pynput.keyboard._xorg',
        'pynput.mouse._darwin',
        'pynput.mouse._win32',
        'pynput.mouse._xorg',
        # PyQt5 components
        'PyQt5.QtCore',
        'PyQt5.QtGui',
        'PyQt5.QtWidgets',
    ],
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    excludes=[],
    win_no_prefer_redirects=False,
    win_private_assemblies=False,
    cipher=block_cipher,
    noarchive=False,
)

pyz = PYZ(a.pure, a.zipped_data, cipher=block_cipher)

exe = EXE(
    pyz,
    a.scripts,
    [],
    exclude_binaries=True,
    name='flow',
    debug=False,
    bootloader_ignore_signals=False,
    strip=False,
    upx=True,
    console=False,
    disable_windowed_traceback=False,
    target_arch=None,
    codesign_identity=None,
    entitlements_file=None,
)

coll = COLLECT(
    exe,
    a.binaries,
    a.zipfiles,
    a.datas,
    strip=False,
    upx=True,
    upx_exclude=[],
    name='flow',
)

if sys.platform == 'darwin':
    app = BUNDLE(
        coll,
        name='flow.app',
        icon=os.path.join(project_root, 'src', 'resources', 'flow.icns') if os.path.exists(os.path.join(project_root, 'src', 'resources', 'flow.icns')) else None,
        bundle_identifier='com.guyavrhm.flow',
        info_plist={
            'NSPrincipalClass': 'NSApplication',
            'NSAppleEventsUsageDescription': 'flow needs to monitor and control mouse and keyboard input.',
            'NSMicrophoneUsageDescription': 'flow does not use the microphone.',
            'NSCameraUsageDescription': 'flow does not use the camera.',
        }
    )

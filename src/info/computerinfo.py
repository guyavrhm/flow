"""
File containing static methods used to obtain machine information.
"""

from sys import platform
import subprocess
import re

WINDOWS = 'win32'
LINUX = 'linux'
MACOS = 'darwin'


def supported():
    """
    Returns True if computer is supported.
    """
    return platform == WINDOWS or platform == LINUX or platform == MACOS


def get_screeninfo():
    """
    Returns screen resolution of computer.
    """
    if platform == WINDOWS:
        output = subprocess.check_output("wmic path Win32_VideoController get CurrentHorizontalResolution,"
                                         "CurrentVerticalResolution", shell=True)
        resolution = re.findall("[0-9]+", output.decode())
        resolutions = [(resolution[i], resolution[i + 1]) for i in range(0, len(resolution) - 1, 2)]

    elif platform == LINUX:
        output = subprocess.check_output("xrandr | grep '*'", shell=True)
        resolution = re.findall("[0-9]+x[0-9]+", output.decode())
        resolutions = [tuple(res.split("x")) for res in resolution]

    elif platform == MACOS:
        output = subprocess.check_output("system_profiler SPDisplaysDataType | grep Resolution", shell=True)
        resolution = re.findall("[0-9]+ x [0-9]+", output.decode())
        resolutions = [tuple(res.split(" x ")) for res in resolution]

    else:
        return None

    return int(resolutions[0][0]), int(resolutions[0][1])  # main screen only


def get_ip():
    """
    Returns local IP of computer.
    """
    if platform == WINDOWS:
        output = subprocess.check_output('ipconfig', shell=True).decode().split('\r\n')
        ips = [output[i - 2].split(': ')[-1] for i, n in enumerate(output) if 'Default Gateway' in n if n[-1].isdigit()]

    elif platform == LINUX:
        output = subprocess.check_output('ip addr', shell=True).decode().split('    ')
        ips = [n.split()[1].split("/")[0] for n in output if 'inet ' in n]
    
    elif platform == MACOS:
        output = subprocess.check_output('ifconfig', shell=True).decode().split('\t')
        ips = [n.split()[1] for n in output if 'inet ' in n]

    else:
        return None

    if len(ips) == 0:
        return 'Not Found'
    else:
        return ips[-1]

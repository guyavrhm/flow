#!/usr/bin/env python3
"""
build.py - Cross-platform installer builder for flow.
Automates compilation, PyInstaller packaging, and installer generation (DMG, tar.gz, EXE).
"""

import os
import sys
import shutil
import subprocess
import tarfile
import zipfile

# Define paths
INSTALLER_DIR = os.path.dirname(os.path.abspath(__file__))
PROJECT_ROOT = os.path.abspath(os.path.join(INSTALLER_DIR, '..'))
DIST_DIR = os.path.join(PROJECT_ROOT, 'dist')
BUILD_DIR = os.path.join(PROJECT_ROOT, 'build')


def print_banner(text):
    print("=" * 60)
    print(f" {text}")
    print("=" * 60)


def check_requirements():
    print("Checking dependencies...")
    
    # Check PyInstaller
    try:
        import PyInstaller
        print(f"  [✓] PyInstaller is installed (version {PyInstaller.__version__})")
    except ImportError:
        print("  [✗] PyInstaller is NOT installed in the current environment.")
        print("      Installing PyInstaller via pip...")
        try:
            subprocess.run([sys.executable, "-m", "pip", "install", "pyinstaller"], check=True)
            import PyInstaller
            print(f"  [✓] PyInstaller installed successfully (version {PyInstaller.__version__})")
        except Exception as e:
            print(f"  [✗] Failed to install PyInstaller: {e}")
            sys.exit(1)

    # Check GCC or Make
    gcc_found = shutil.which("gcc") is not None
    make_found = shutil.which("make") is not None
    
    if not gcc_found:
        print("  [✗] GCC compiler not found. You will need a C compiler to compile the AES extension.")
    else:
        print("  [✓] GCC compiler found.")
        
    if not make_found:
        print("  [i] 'make' utility not found. Will compile manually using gcc.")
    else:
        print("  [✓] 'make' utility found.")


def prepare_icons():
    print_banner("Preparing Application Icons")
    
    # 1. Copy Windows Icon if present
    src_ico = os.path.join(PROJECT_ROOT, 'docs', 'images', 'flow.ico')
    dest_ico = os.path.join(PROJECT_ROOT, 'src', 'resources', 'flow.ico')
    if os.path.exists(src_ico):
        print(f"Copying Windows icon to: {dest_ico}")
        shutil.copy(src_ico, dest_ico)
    else:
        print("Warning: docs/images/flow.ico not found. Shortcut icons for Windows might be default.")

    # 2. Compile macOS ICNS file if on macOS
    if sys.platform == 'darwin':
        src_png = os.path.join(PROJECT_ROOT, 'src', 'resources', 'flow.png')
        dest_icns = os.path.join(PROJECT_ROOT, 'src', 'resources', 'flow.icns')
        
        if os.path.exists(src_png):
            print(f"Generating macOS ICNS icon at: {dest_icns}")
            iconset_dir = os.path.join(PROJECT_ROOT, 'src', 'resources', 'flow.iconset')
            os.makedirs(iconset_dir, exist_ok=True)
            
            sizes = [16, 32, 64, 128, 256, 512, 1024]
            try:
                for size in sizes:
                    out_png = os.path.join(iconset_dir, f"icon_{size}x{size}.png")
                    subprocess.run(["sips", "-z", str(size), str(size), src_png, "--out", out_png], capture_output=True, check=True)
                    if size * 2 <= 1024:
                        out_png_2x = os.path.join(iconset_dir, f"icon_{size}x{size}@2x.png")
                        subprocess.run(["sips", "-z", str(size*2), str(size*2), src_png, "--out", out_png_2x], capture_output=True, check=True)
                
                # Compile to icns
                subprocess.run(["iconutil", "-c", "icns", iconset_dir, "-o", dest_icns], capture_output=True, check=True)
                print("macOS ICNS icon generated successfully!")
            except Exception as e:
                print(f"Failed to generate macOS ICNS icon: {e}")
            finally:
                if os.path.exists(iconset_dir):
                    shutil.rmtree(iconset_dir)
        else:
            print("Warning: src/resources/flow.png not found. Cannot generate macOS ICNS icon.")


def compile_c_library():
    print_banner("Compiling AES C Extension")
    
    c_src_dir = os.path.join(PROJECT_ROOT, 'src', 'network', 'aes')
    aes_c = os.path.join(c_src_dir, 'aes.c')
    gmult_c = os.path.join(c_src_dir, 'gmult.c')
    
    if sys.platform == 'win32':
        output_dll = os.path.join(c_src_dir, 'aes.dll')
        print(f"Compiling Windows DLL -> {output_dll}")
        cmd = ["gcc", "-fPIC", "-shared", "-o", output_dll, aes_c, gmult_c]
    else:
        output_so = os.path.join(c_src_dir, 'aes.so')
        print(f"Compiling Unix Shared Library -> {output_so}")
        # Try make first if available
        if shutil.which("make") is not None:
            cmd = ["make"]
        else:
            cmd = ["gcc", "-fPIC", "-shared", "-o", output_so, aes_c, gmult_c]

    print(f"Running command: {' '.join(cmd)}")
    try:
        # Run from root folder so Makefile rules resolve correctly if using make
        subprocess.run(cmd, cwd=PROJECT_ROOT, check=True)
        print("C compilation completed successfully!")
    except Exception as e:
        print(f"Error during C library compilation: {e}")
        print("Ensure you have a working C compiler (GCC/Clang/MSVC) installed and in your PATH.")
        sys.exit(1)


def run_pyinstaller():
    print_banner("Running PyInstaller Packaging")
    
    spec_file = os.path.join(INSTALLER_DIR, 'flow.spec')
    cmd = ["pyinstaller", "--noconfirm", spec_file]
    
    print(f"Running PyInstaller with spec: {spec_file}")
    try:
        subprocess.run(cmd, cwd=PROJECT_ROOT, check=True)
        print("PyInstaller bundling completed successfully!")
    except subprocess.CalledProcessError as e:
        print(f"PyInstaller execution failed: {e}")
        sys.exit(1)


def build_macos_installer():
    print_banner("Building macOS DMG Installer")
    
    app_path = os.path.join(DIST_DIR, 'flow.app')
    temp_dmg = os.path.join(DIST_DIR, 'temp.dmg')
    dmg_path = os.path.join(DIST_DIR, 'flow.dmg')
    
    if not os.path.exists(app_path):
        print(f"Error: flow.app not found at '{app_path}'")
        return
        
    if os.path.exists(dmg_path):
        print(f"Removing old DMG: {dmg_path}")
        os.remove(dmg_path)
    if os.path.exists(temp_dmg):
        os.remove(temp_dmg)
        
    # Step 1: Create a temporary read-write DMG
    print("Creating temporary read-write DMG...")
    # Estimate size in MB (add 30MB overhead for safety)
    app_size_bytes = sum(os.path.getsize(os.path.join(dirpath, filename)) for dirpath, _, filenames in os.walk(app_path) for filename in filenames)
    app_size_mb = int(app_size_bytes / (1024 * 1024)) + 30
    
    cmd = [
        "hdiutil", "create",
        "-size", f"{app_size_mb}m",
        "-volname", "flow",
        "-fs", "HFS+",
        temp_dmg
    ]
    subprocess.run(cmd, check=True)
    
    # Step 2: Mount the DMG
    print("Mounting DMG...")
    mount_cmd = ["hdiutil", "attach", "-readwrite", "-noverify", "-noautoopen", temp_dmg]
    res = subprocess.run(mount_cmd, capture_output=True, text=True, check=True)
    
    # Find mount path (usually /Volumes/flow)
    mount_path = None
    for line in res.stdout.splitlines():
        if "/Volumes/flow" in line:
            mount_path = line.split("\t")[-1].strip()
            break
            
    if not mount_path:
        mount_path = "/Volumes/flow"
        
    print(f"Mounted at: {mount_path}")
    
    try:
        # Step 3: Copy Files
        print("Copying app bundle...")
        subprocess.run(["cp", "-R", app_path, os.path.join(mount_path, 'flow.app')], check=True)
        
        print("Creating Applications symlink...")
        os.symlink('/Applications', os.path.join(mount_path, 'Applications'))
        
    except Exception as e:
        print(f"Error while copying files to DMG: {e}")
    finally:
        # Step 4: Unmount DMG
        print("Unmounting DMG...")
        subprocess.run(["sync"])
        subprocess.run(["hdiutil", "detach", mount_path], check=True)
        
    # Step 5: Convert to final compressed format
    print("Converting DMG to compressed production image...")
    convert_cmd = [
        "hdiutil", "convert",
        temp_dmg,
        "-format", "UDZO",
        "-imagekey", "zlib-level=9",
        "-o", dmg_path
    ]
    subprocess.run(convert_cmd, check=True)
    print(f"macOS production DMG created successfully at: {dmg_path}")
    
    # Cleanup
    if os.path.exists(temp_dmg):
        os.remove(temp_dmg)


def build_linux_installer():
    print_banner("Building Linux tar.gz Installer")
    
    flow_dist_dir = os.path.join(DIST_DIR, 'flow')
    tar_path = os.path.join(DIST_DIR, 'flow.tar.gz')
    
    if not os.path.exists(flow_dist_dir):
        print(f"Error: Build folder not found at '{flow_dist_dir}'")
        return
        
    if os.path.exists(tar_path):
        print(f"Removing old tar.gz: {tar_path}")
        os.remove(tar_path)
        
    print(f"Compressing application into: {tar_path}")
    
    try:
        with tarfile.open(tar_path, "w:gz") as tar:
            # Add setup script and desktop file to the root of the archive
            setup_script = os.path.join(INSTALLER_DIR, 'setup_linux.sh')
            desktop_file = os.path.join(INSTALLER_DIR, 'flow.desktop')
            
            if os.path.exists(setup_script):
                tar.add(setup_script, arcname='setup.sh')
            if os.path.exists(desktop_file):
                tar.add(desktop_file, arcname='flow.desktop')
                
            # Add the bundled flow directory under the subdirectory 'flow'
            tar.add(flow_dist_dir, arcname='flow')
            
        print(f"Linux tar.gz package created successfully at: {tar_path}")
    except Exception as e:
        print(f"Failed to create tar.gz package: {e}")


def build_windows_installer():
    print_banner("Building Windows Setup Installer")
    
    flow_dist_dir = os.path.join(DIST_DIR, 'flow')
    if not os.path.exists(flow_dist_dir):
        print(f"Error: Build folder not found at '{flow_dist_dir}'")
        return
        
    # Look for Inno Setup compiler (ISCC)
    iscc_path = shutil.which("ISCC")
    if not iscc_path:
        # Check standard path
        standard_iscc = r"C:\Program Files (x86)\Inno Setup 6\ISCC.exe"
        if os.path.exists(standard_iscc):
            iscc_path = standard_iscc

    if iscc_path:
        print(f"Found Inno Setup compiler: {iscc_path}")
        iss_file = os.path.join(INSTALLER_DIR, 'flow.iss')
        cmd = [iscc_path, iss_file]
        try:
            subprocess.run(cmd, check=True)
            print("Windows EXE Setup created successfully via Inno Setup!")
            return
        except Exception as e:
            print(f"Inno Setup compilation failed: {e}")
            
    # Fallback to ZIP archive if Inno Setup isn't available
    zip_path = os.path.join(DIST_DIR, 'flow-windows.zip')
    print("Inno Setup (ISCC.exe) not found. Falling back to ZIP package creation...")
    if os.path.exists(zip_path):
        os.remove(zip_path)
        
    try:
        with zipfile.ZipFile(zip_path, 'w', zipfile.ZIP_DEFLATED) as zipf:
            for root, dirs, files in os.walk(flow_dist_dir):
                for file in files:
                    full_path = os.path.join(root, file)
                    rel_path = os.path.relpath(full_path, PROJECT_ROOT)
                    zipf.write(full_path, arcname=rel_path)
        print(f"Windows ZIP package created successfully at: {zip_path}")
    except Exception as e:
        print(f"Failed to create Windows ZIP package: {e}")


def main():
    check_requirements()
    prepare_icons()
    compile_c_library()
    run_pyinstaller()
    
    # Generate OS-specific installers
    if sys.platform == 'darwin':
        build_macos_installer()
    elif sys.platform == 'win32':
        build_windows_installer()
    elif sys.platform.startswith('linux'):
        build_linux_installer()
    else:
        print(f"Unsupported OS for packaging: {sys.platform}")

    print_banner("Build Summary")
    print(f"Build outputs generated in: {DIST_DIR}")
    for item in os.listdir(DIST_DIR):
        print(f" - {item}")


if __name__ == '__main__':
    main()

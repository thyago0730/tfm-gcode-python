# -*- mode: python ; coding: utf-8 -*-


from PyInstaller.utils.hooks import collect_submodules
from PyInstaller.building.build_main import Analysis, PYZ, EXE
from PyInstaller.utils.hooks import collect_data_files
from PyInstaller.building.datastruct import Tree

a = Analysis(
    ['src/app/TFM_GCODE.py'],
    pathex=['.'],
    binaries=[],
    datas=[
        ('assets/logo.png', 'assets'),
        ('config/config.json', '.'),
        Tree('data/procedures', prefix='procedures'),
    ],
    hiddenimports=[],
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    excludes=[],
    noarchive=False,
    optimize=0,
)
pyz = PYZ(a.pure)

exe = EXE(
    pyz,
    a.scripts,
    a.binaries,
    a.datas,
    [],
    name='TFM_GCODE',
    debug=False,
    bootloader_ignore_signals=False,
    strip=False,
    upx=True,
    upx_exclude=[],
    runtime_tmpdir=None,
    console=False,
    disable_windowed_traceback=False,
    argv_emulation=False,
    target_arch=None,
    codesign_identity=None,
    entitlements_file=None,

)

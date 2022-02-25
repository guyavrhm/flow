@echo off
powershell -c "Get-CimInstance -ClassName Win32_VideoController | Select-Object CurrentHorizontalResolution, CurrentVerticalResolution"

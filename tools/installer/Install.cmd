@echo off
rem Polylinker installer entry point.
rem
rem This exists only because double-clicking a .ps1 does not run it: the default
rem execution policy on Windows client editions is Restricted, and a script that
rem Explorer extracted from a downloaded zip additionally carries a
rem Mark-of-the-Web that RemoteSigned rejects. -ExecutionPolicy Bypass is how a
rem script is run deliberately; it is not a security hole being opened, because
rem execution policy is not a security boundary and Microsoft says so. The thing
rem protecting you here is that Install-Polylinker.ps1 is beside this file, is
rem plain text, prints everything it intends to do, and waits for you to type
rem yes.
rem
rem Read that file first. It is meant to be read.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0Install-Polylinker.ps1" %*
pause

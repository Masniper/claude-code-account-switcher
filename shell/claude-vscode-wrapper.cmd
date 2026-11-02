@echo off
REM Claude Code VS Code Extension Wrapper for Windows
REM Resolves account from working directory, then invokes real claude.exe

setlocal enabledelayedexpansion

REM Get account for current directory
for /f "delims=" %%i in ('"%USERPROFILE%\.claude-switch\bin\claude-acc.exe" activate --shell powershell') do set ACTIVATION=%%i

REM Parse CLAUDE_CONFIG_DIR from activation output
for /f "tokens=1,2 delims==" %%a in ("%ACTIVATION%") do (
    if "%%a"=="$env:CLAUDE_CONFIG_DIR" set CLAUDE_CONFIG_DIR=%%b
)

REM Remove quotes if present
set CLAUDE_CONFIG_DIR=%CLAUDE_CONFIG_DIR:"=%

REM Invoke real claude binary from VS Code extension
REM First argument from vscode is the extension's bundled claude.exe path
set REAL_CLAUDE=%1
shift

REM Pass remaining args
%REAL_CLAUDE% %*

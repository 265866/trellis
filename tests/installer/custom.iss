; The release/installer-custom.iss run.sh builds the demo binary's installers with, shaped like a
; real program's, arcwise's: defines, an icon, entries in several sections, and [Code] last, where
; a `;` line isn't a comment.

; Created by the program while it runs, which the uninstaller below waits for.
#define RunningMutex "DemoBinary.Running"
; The mutex an earlier installer gave setup.
#define SetupMutex "DemoBinary.Setup"

[Setup]
SetupIconFile=demo.ico

[InstallDelete]
; A file an older version installed.
Type: files; Name: "{app}\LICENSE.txt"

[UninstallDelete]
; Installers the program downloaded to update itself.
Type: filesandordirs; Name: "{localappdata}\DemoBinary\updates"
Type: dirifempty; Name: "{localappdata}\DemoBinary"

[Run]
Filename: "{app}\demo-binary.exe"; Parameters: "--version"; Flags: nowait postinstall skipifsilent
; The program's updater installs silently with /RELAUNCH to start the new version afterwards.
Filename: "{app}\demo-binary.exe"; Parameters: "--version"; Flags: nowait; Check: WizardSilent and RelaunchRequested

[Code]
function RelaunchRequested: Boolean;
var
  I: Integer;
begin
  Result := False;
  for I := 1 to ParamCount do
    if CompareText(ParamStr(I), '/RELAUNCH') = 0 then
      Result := True;
end;

// The uninstaller can't remove the program while it runs, so it asks for it to be closed first.
function InitializeUninstall: Boolean;
begin
  Result := True;
  while Result and CheckForMutexes('{#RunningMutex}') do
  begin
    if UninstallSilent then
      Result := False
    else
      Result := MsgBox('Demo Binary is running. Close it, then click OK to continue uninstalling.',
        mbError, MB_OKCANCEL) = IDOK;
  end;
end;

// Settings stay by default, so reinstalling picks up where the program left off.
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if (CurUninstallStep = usPostUninstall) and not UninstallSilent then
    if MsgBox('Also delete your Demo Binary settings?', mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES then
      DelTree(ExpandConstant('{userappdata}\DemoBinary'), True, True, True);
end;

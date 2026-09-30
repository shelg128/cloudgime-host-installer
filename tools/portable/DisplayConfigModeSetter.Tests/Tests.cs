using System.Runtime.InteropServices;
using static DisplayPrepareHelper.DisplayConfigModeSetter;
int count=0;void Check(bool b,string n){if(!b)throw new Exception(n);Console.WriteLine("PASS "+n);count++;}
Snapshot Fixture()=>new(new[]{new DisplayPath{SourceAdapter=11,SourceId=2,SourceIndex=0,TargetAdapter=11,TargetId=3,TargetIndex=2,Rotation=1,RefreshNumerator=60,RefreshDenominator=1,Flags=1},new DisplayPath{SourceAdapter=22,SourceId=4,SourceIndex=1,TargetAdapter=22,TargetId=5,Rotation=1,RefreshNumerator=60,RefreshDenominator=1,Flags=1}},new[]{new DisplayMode{Type=1,Id=2,Adapter=11,Width=1280,Height=720,PixelFormat=4},new DisplayMode{Type=1,Id=4,Adapter=22,Width=1920,Height=1080,X=1280,PixelFormat=4},new DisplayMode{Type=2,Id=3,Adapter=11,Data0=123,Data5=456}});
Check(Marshal.SizeOf<DisplayPath>()==72&&Marshal.SizeOf<DisplayMode>()==64,"native CCD ABI sizes");
var before=Fixture();var p=MakePlan(before,0,720,1570,30);Check(before.Modes[0].Width==1280&&p.Desired.Modes[0].Width==720&&p.Desired.Modes[0].Height==1570,"plan clones snapshot, portrait exact");
Check(p.Desired.Paths[0].TargetIndex==InvalidModeIndex&&p.Desired.Paths[0].Rotation==1,"native portrait timing requested without rotation hack");
Check(p.Desired.Modes[1].Equals(before.Modes[1])&&p.Desired.Paths[1].Equals(before.Paths[1])&&p.Desired.Modes[2].Equals(before.Modes[2]),"other display and union bytes preserved");
Check(Matches(p,p.Desired),"matching actual geometry accepted");
Check(!Matches(p,before),"old landscape not accepted as portrait");
var changed=Fixture();changed.Modes[0]=p.Desired.Modes[0];changed.Paths[0]=p.Desired.Paths[0];changed.Modes[1].Width=1280;Check(!Matches(p,changed),"unrelated monitor change rejected");
var alias=Fixture();alias.Paths[1].SourceAdapter=11;alias.Paths[1].SourceId=2;try{MakePlan(alias,0,720,1570,30);throw new Exception("clone allowed");}catch(InvalidDataException){}Check(true,"clone source cannot gain exclusive authority");
foreach(var dims in new[]{(721,1570,30),(0,1570,30),(720,1570,1000)}){try{MakePlan(Fixture(),0,dims.Item1,dims.Item2,dims.Item3);throw new Exception("invalid allowed");}catch(InvalidDataException){}}Check(true,"invalid size/rate rejected before OS mutation");
var broken=Fixture();broken.Paths[0].SourceIndex=500;try{MakePlan(broken,0,720,1570,30);throw new Exception("index allowed");}catch(InvalidDataException){}Check(true,"untrusted native mode index bounded");
Console.WriteLine("CCD_MODE_TESTS_PASS "+count);

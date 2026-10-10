using DisplayPrepareHelper;
long now=0;int reads=0;
bool a=DisplayModeStability.Verify(720,1570,()=>{reads++;return(720,1570);},ms=>now+=ms,()=>now);
if(!a||now!=6000)throw new Exception("stable");Console.WriteLine("PASS stable mode observed without writes");
now=0;bool b=DisplayModeStability.Verify(720,1570,()=>now<2750?(720,1570):(1280,720),ms=>now+=ms,()=>now);
if(b||now!=2750)throw new Exception("reset");Console.WriteLine("PASS external mode reset rejects success");
now=0;if(DisplayModeStability.Verify(720,1570,()=>null,ms=>now+=ms,()=>now))throw new Exception("missing");Console.WriteLine("PASS display disappearance rejected");
now=0;if(DisplayModeStability.Verify(720,1570,()=>(1570,720),ms=>now+=ms,()=>now))throw new Exception("rotation");Console.WriteLine("PASS wrong orientation rejected");

import android.media.*;
import android.os.*;
import android.view.Surface;
import android.graphics.SurfaceTexture;
import android.opengl.*;
import org.json.*;
import java.util.*;
import java.util.concurrent.atomic.AtomicBoolean;

public class DecoderReplay {
 static long now(){return SystemClock.elapsedRealtimeNanos();}
 static String chosen="c2.unisoc.avc.decoder";
 static MediaCodecInfo codecInfo() throws Exception {for(MediaCodecInfo i:new MediaCodecList(MediaCodecList.ALL_CODECS).getCodecInfos())if(i.getName().equals(chosen))return i;throw new Exception("EXACT_CODEC_MISSING");}
 static class Target implements AutoCloseable {
  EGLDisplay d; EGLContext c; EGLSurface e; SurfaceTexture texture; Surface surface; HandlerThread events; AtomicBoolean pending=new AtomicBoolean();long callbacks;
  Target() throws Exception {
   d=EGL14.eglGetDisplay(EGL14.EGL_DEFAULT_DISPLAY);int[] v=new int[2];if(!EGL14.eglInitialize(d,v,0,v,1))throw new Exception("EGL_INIT");EGLConfig[] cfg=new EGLConfig[1];int[] n=new int[1];int[] a={EGL14.EGL_RED_SIZE,8,EGL14.EGL_GREEN_SIZE,8,EGL14.EGL_BLUE_SIZE,8,EGL14.EGL_RENDERABLE_TYPE,4,EGL14.EGL_SURFACE_TYPE,EGL14.EGL_PBUFFER_BIT,EGL14.EGL_NONE};if(!EGL14.eglChooseConfig(d,a,0,cfg,0,1,n,0)||n[0]!=1)throw new Exception("EGL_CONFIG");c=EGL14.eglCreateContext(d,cfg[0],EGL14.EGL_NO_CONTEXT,new int[]{EGL14.EGL_CONTEXT_CLIENT_VERSION,2,EGL14.EGL_NONE},0);e=EGL14.eglCreatePbufferSurface(d,cfg[0],new int[]{EGL14.EGL_WIDTH,1,EGL14.EGL_HEIGHT,1,EGL14.EGL_NONE},0);if(!EGL14.eglMakeCurrent(d,e,e,c))throw new Exception("EGL_CURRENT");int[] t=new int[1];GLES20.glGenTextures(1,t,0);texture=new SurfaceTexture(t[0]);events=new HandlerThread("replay-surface");events.start();texture.setOnFrameAvailableListener(x->{pending.set(true);},new Handler(events.getLooper()));surface=new Surface(texture);
  }
  Long drain(){if(!pending.getAndSet(false))return null;texture.updateTexImage();callbacks++;return texture.getTimestamp();}
  public void close(){if(surface!=null)surface.release();if(texture!=null)texture.release();if(events!=null)events.quitSafely();if(d!=null){EGL14.eglMakeCurrent(d,EGL14.EGL_NO_SURFACE,EGL14.EGL_NO_SURFACE,EGL14.EGL_NO_CONTEXT);EGL14.eglDestroySurface(d,e);EGL14.eglDestroyContext(d,c);EGL14.eglTerminate(d);}}
 }
 static JSONObject run(String file,boolean low,int gapMs,int ordinal) throws Exception {
  JSONObject out=new JSONObject();out.put("lowLatencyRequested",low);out.put("gapMs",gapMs);out.put("ordinal",ordinal);JSONArray inputs=new JSONArray(),outputs=new JSONArray(),surfaces=new JSONArray();out.put("inputs",inputs);out.put("outputs",outputs);out.put("surfaces",surfaces);MediaExtractor ex=new MediaExtractor();MediaCodec codec=null;Target target=null;
  try{ex.setDataSource(file);ex.selectTrack(0);MediaFormat fmt=ex.getTrackFormat(0);out.put("inputFormat",fmt.toString());if(!fmt.getString(MediaFormat.KEY_MIME).equals("video/avc"))throw new Exception("H264_REQUIRED");if(low)fmt.setInteger("low-latency",1);target=new Target();codec=MediaCodec.createByCodecName(chosen);codec.configure(fmt,target.surface,null,0);codec.start();long start=now(),next=0,endInputs=0;boolean allInput=false,eosQueued=false,eos=false;int index=0,maxOutstanding=0;MediaCodec.BufferInfo info=new MediaCodec.BufferInfo();HashMap<Long,Long> queued=new HashMap<>();
   while(now()-start<12_000_000_000L){Long surfaceTs=target.drain();if(surfaceTs!=null)surfaces.put(new JSONObject().put("atNs",now()).put("stampNs",surfaceTs));int oi;while((oi=codec.dequeueOutputBuffer(info,0))>=0){long at=now();Long in=queued.remove(info.presentationTimeUs);boolean end=(info.flags&MediaCodec.BUFFER_FLAG_END_OF_STREAM)!=0;outputs.put(new JSONObject().put("atNs",at).put("ptsUs",info.presentationTimeUs).put("size",info.size).put("flags",info.flags).put("residenceMs",in==null?JSONObject.NULL:(at-in)/1e6).put("afterAllInput",allInput));codec.releaseOutputBuffer(oi,!end);if(end){eos=true;break;} }if(oi==MediaCodec.INFO_OUTPUT_FORMAT_CHANGED)out.put("outputFormat",codec.getOutputFormat().toString());if(eos)break;long elapsed=now()-start;
    if(!allInput&&ex.getSampleTime()<0){allInput=true;endInputs=now();}
    if(!allInput&&elapsed>=next){int ii=codec.dequeueInputBuffer(0);if(ii>=0){java.nio.ByteBuffer b=codec.getInputBuffer(ii);int sz=ex.readSampleData(b,0);if(sz<0){allInput=true;endInputs=now();codec.queueInputBuffer(ii,0,0,0,0);}else{long pts=ex.getSampleTime(),at=now();codec.queueInputBuffer(ii,0,sz,pts,0);queued.put(pts,at);inputs.put(new JSONObject().put("atNs",at).put("ptsUs",pts).put("bytes",sz).put("index",index));ex.advance();index++;next=index*33_333_333L+(index>=30?gapMs*1_000_000L:0);maxOutstanding=Math.max(maxOutstanding,queued.size());}}}
    if(allInput&&!eosQueued&&now()-endInputs>=400_000_000L){int ii=codec.dequeueInputBuffer(0);if(ii>=0){codec.queueInputBuffer(ii,0,0,index*33333L,MediaCodec.BUFFER_FLAG_END_OF_STREAM);eosQueued=true;out.put("eosInputNs",now());}}
    Thread.sleep(1);
   }out.put("eos",eos);out.put("maxOutstanding",maxOutstanding);out.put("endNs",now());out.put("metrics",codec.getMetrics().toString());out.put("surfaceCallbacks",target.callbacks);out.put("configuredFormat",fmt.toString());
  }catch(Exception e){out.put("error",e.getClass().getName()+":"+e.getMessage());}finally{if(codec!=null){try{codec.stop();}catch(Exception ignored){}codec.release();}ex.release();if(target!=null)target.close();}return out;
 }
 public static void main(String[] args)throws Exception {JSONObject result=new JSONObject();MediaCodecInfo i=codecInfo();MediaCodecInfo.CodecCapabilities cap=i.getCapabilitiesForType("video/avc");result.put("codec",chosen);result.put("sdk",Build.VERSION.SDK_INT);result.put("lowLatencySupported",cap.isFeatureSupported("low-latency"));result.put("model",Build.MODEL);if(args.length==0){System.out.println(result.toString());return;}JSONArray runs=new JSONArray();result.put("runs",runs);for(int rep=0;rep<3;rep++)for(int g:new int[]{0,300})for(boolean low:new boolean[]{rep%2!=0,rep%2==0}){runs.put(run(args[0],low,g,rep));try(java.io.FileWriter f=new java.io.FileWriter(args[1])){f.write(result.toString());}}System.out.println(new JSONObject().put("finished",true).put("runs",runs.length()).put("output",args[1]).toString());}
}

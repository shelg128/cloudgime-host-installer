import android.graphics.Bitmap;
import android.graphics.Matrix;
import android.os.SystemClock;
import org.json.JSONObject;
import java.io.*;
import java.nio.*;
import java.security.MessageDigest;
import java.util.concurrent.*;

/** QA-only app_process helper: no APK, network, microphone, secure-layer capture or unlock. */
public final class FreshThumbnail {
 private static final PrintStream OUT=new PrintStream(new FileOutputStream(FileDescriptor.out));
 private static final int MAX_RAW=32*1024*1024,MAX_JPEG=1024*1024;
 private static volatile Process capture;
 private static final String READY="CG_THUMB_READY_V1 ",IMAGE="CG_THUMB_IMAGE_V1 ",ERROR="CG_THUMB_ERROR_V1 ";
 private static void line(String text){OUT.println(text);OUT.flush();}
 private static byte[] bounded(InputStream in,int limit)throws IOException {ByteArrayOutputStream out=new ByteArrayOutputStream();byte[] b=new byte[65536];int n;while((n=in.read(b))!=-1){if(out.size()+n>limit)throw new IOException("BYTE_LIMIT");out.write(b,0,n);}return out.toByteArray();}
 private static int le(byte[] b,int off){return ByteBuffer.wrap(b,off,4).order(ByteOrder.LITTLE_ENDIAN).getInt();}
 private static Bitmap decodeRaw(byte[] raw)throws Exception {
  if(raw.length<16)throw new IOException("RAW_HEADER");int w=le(raw,0),h=le(raw,4),format=le(raw,8);
  if(w<=0||h<=0||w>8192||h>8192||(long)w*h*4>MAX_RAW-16)throw new IOException("DIMENSIONS");
  // Android 16 screencap raw: width,height,pixel-format,dataspace then tightly packed rows.
  // Other layouts/formats are rejected; there is no guessed stride or fallback capture.
  if(format!=1||raw.length!=16+(long)w*h*4)throw new IOException("UNSUPPORTED_RAW_LAYOUT");
  Bitmap bitmap=Bitmap.createBitmap(w,h,Bitmap.Config.ARGB_8888);
  bitmap.copyPixelsFromBuffer(ByteBuffer.wrap(raw,16,raw.length-16));return bitmap;
 }
 private static String sha(byte[] b)throws Exception {byte[] d=MessageDigest.getInstance("SHA-256").digest(b);StringBuilder out=new StringBuilder();for(byte v:d)out.append(String.format(java.util.Locale.ROOT,"%02x",v&255));return out.toString();}
 private static String readRequest()throws Exception {ExecutorService reader=Executors.newSingleThreadExecutor(r->{Thread t=new Thread(r,"cg-thumb-request");t.setDaemon(true);return t;});try{return reader.submit(()->{InputStream in=new FileInputStream(FileDescriptor.in);ByteArrayOutputStream bytes=new ByteArrayOutputStream();for(;;){int b=in.read();if(b<0)throw new EOFException("NO_REQUEST");if(b==10)return new String(bytes.toByteArray(),"US-ASCII");if(bytes.size()>=64)throw new IOException("REQUEST_LIMIT");bytes.write(b);}}).get(10000,TimeUnit.MILLISECONDS);}finally{reader.shutdownNow();}}
 private static Bitmap selfTestBitmap()throws Exception {
  int w=8,h=4;byte[] raw=new byte[16+w*h*4];ByteBuffer head=ByteBuffer.wrap(raw).order(ByteOrder.LITTLE_ENDIAN);head.putInt(w).putInt(h).putInt(1).putInt(0);
  int[] colors={0xffff0000,0xff00ff00,0xff0000ff,0xffffff00};
  for(int y=0;y<h;y++)for(int x=0;x<w;x++){int c=colors[x/2],p=16+(y*w+x)*4;raw[p]=(byte)(c>>16);raw[p+1]=(byte)(c>>8);raw[p+2]=(byte)c;raw[p+3]=(byte)255;}
  Bitmap b=decodeRaw(raw);for(int x=0;x<w;x++)if(b.getPixel(x,0)!=colors[x/2])throw new IOException("COLOR_ORDER");return b;
 }
 public static void main(String[]args){
  Thread watchdog=new Thread(()->{try{Thread.sleep(20000);}catch(Exception ignored){}Process p=capture;if(p!=null)p.destroy();System.exit(3);},"cg-thumb-lifetime");watchdog.setDaemon(true);watchdog.start();
  Bitmap bitmap=null,small=null;int exit=0;
  try{
   if(args.length==1&&args[0].equals("--self-test")){bitmap=selfTestBitmap();line("CG_THUMB_SELFTEST_V1 {\"passed\":true,\"colors\":4,\"captureCount\":0}");return;}
   if(args.length!=1||!args[0].matches("[a-f0-9]{32}"))throw new IOException("NONCE");String nonce=args[0];
   line(READY+new JSONObject().put("schema",1).put("nonce",nonce).put("idleTimeoutMs",10000).put("lifetimeMs",20000).put("maxCaptureMs",8000).toString());
   String request=readRequest();if(!request.equals("CAPTURE "+nonce))throw new IOException("REQUEST_SCOPE");long begin=SystemClock.elapsedRealtimeNanos();
   capture=new ProcessBuilder("/system/bin/screencap").redirectError(ProcessBuilder.Redirect.INHERIT).start();
   byte[] raw=bounded(capture.getInputStream(),MAX_RAW);if(!capture.waitFor(1,TimeUnit.SECONDS)||capture.exitValue()!=0)throw new IOException("SCREENCAP_EXIT");capture=null;
   long acquired=SystemClock.elapsedRealtimeNanos();bitmap=decodeRaw(raw);int w=bitmap.getWidth(),h=bitmap.getHeight(),width=Math.min(480,w),height=Math.max(1,(int)Math.round(h*(width/(double)w)));
   small=Bitmap.createScaledBitmap(bitmap,width,height,true);ByteArrayOutputStream bytes=new ByteArrayOutputStream();if(!small.compress(Bitmap.CompressFormat.JPEG,80,bytes))throw new IOException("JPEG_ENCODE");byte[] jpeg=bytes.toByteArray();if(jpeg.length<256||jpeg.length>MAX_JPEG)throw new IOException("JPEG_SIZE");
   long ended=SystemClock.elapsedRealtimeNanos();if((ended-begin)/1000000>8000)throw new IOException("CAPTURE_DEADLINE");
   JSONObject meta=new JSONObject().put("schema",1).put("nonce",nonce).put("bytes",jpeg.length).put("sha256",sha(jpeg)).put("sourceWidth",w).put("sourceHeight",h).put("width",width).put("height",height).put("pixelFormat",le(raw,8)).put("dataspace",le(raw,12)).put("rawBytes",raw.length).put("captureCount",1).put("acquisitionMs",(acquired-begin)/1e6).put("deviceTotalMs",(ended-begin)/1e6).put("startedMonoNs",Long.toString(begin)).put("finishedMonoNs",Long.toString(ended));
   String encoded=java.util.Base64.getEncoder().encodeToString(jpeg);meta.put("encoding","base64").put("encodedBytes",encoded.length());
   line(IMAGE+meta.toString());line(encoded);
  }catch(Throwable e){exit=2;try{line(ERROR+new JSONObject().put("error",e.getClass().getSimpleName()).put("message",String.valueOf(e.getMessage()).replaceAll("[^A-Za-z0-9_ :]","_")).toString());}catch(Exception ignored){}}
  finally{if(capture!=null)capture.destroy();if(small!=null&&small!=bitmap)small.recycle();if(bitmap!=null)bitmap.recycle();if(exit!=0)System.exit(exit);}
 }
}

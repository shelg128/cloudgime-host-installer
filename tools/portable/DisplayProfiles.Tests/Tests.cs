using System.Xml.Linq;
namespace DisplayPrepareHelper;
internal static class DisplayProfileTests {
 static int count;
 static void Check(bool v,string name){if(!v)throw new Exception(name);Console.WriteLine("PASS "+name);count++;}
 static void Main(){
 var doc=XDocument.Parse("<vdd_settings><gpu><friendlyname>preserved</friendlyname></gpu><resolutions><resolution><width>1280</width><height>720</height><refresh_rate>60</refresh_rate></resolution></resolutions><options custom='untouched'/></vdd_settings>");
 var r=doc.Root!.Element("resolutions")!;
 Check(DisplayModeCatalog.AddDeviceOrientations(r,720,1570,30),"first device adds both orientations");
 var snapshot=doc.ToString();Check(!DisplayModeCatalog.AddDeviceOrientations(r,1570,720,30)&&snapshot==doc.ToString(),"rotation reuses modes without rewriting");
 var loaded=XDocument.Parse(snapshot);Check(!DisplayModeCatalog.AddDeviceOrientations(loaded.Root!.Element("resolutions")!,720,1570,30),"new process uses persisted mode catalog");
 Check(DisplayModeCatalog.AddDeviceOrientations(r,1080,2400,60),"second device adds independently");
 Check(!DisplayModeCatalog.AddDeviceOrientations(r,720,1570,30),"first device still registered after second");
 Check(doc.Root.Element("gpu")!.Value=="preserved"&&doc.Root.Element("options")!.Attribute("custom")!.Value=="untouched","unrelated settings preserved");
 Check(DisplayModeCatalog.AddDeviceOrientations(r,1024,1024,60)&&r.Elements().Count(x=>x.Element("width")!.Value=="1024")==1,"square device deduplicates orientation");
 var invalid=r.ToString();try{DisplayModeCatalog.AddDeviceOrientations(r,721,1570,30);throw new Exception("invalid accepted");}catch(InvalidDataException){}Check(invalid==r.ToString(),"invalid geometry rejects before mutation");
 var full=new XElement("resolutions",Enumerable.Range(0,DisplayModeCatalog.Capacity).Select(i=>new XElement("resolution",new XElement("width",320+i*2),new XElement("height",720),new XElement("refresh_rate",30))));
 var before=full.ToString();try{DisplayModeCatalog.AddDeviceOrientations(full,1080,2400,60);throw new Exception("full accepted");}catch(InvalidDataException){}Check(before==full.ToString(),"capacity fails atomically without evicting devices");
 Check(!DisplayModeCatalog.AddDeviceOrientations(new XElement("resolutions",new XElement("resolution",new XElement("width",640),new XElement("height",640),new XElement("refresh_rate",60))),640,640,60),"existing square requires no registration");
 Console.WriteLine("DISPLAY_PROFILE_TESTS_PASS "+count);
 }
}

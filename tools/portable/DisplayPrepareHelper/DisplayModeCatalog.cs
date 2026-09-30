using System.Xml.Linq;
namespace DisplayPrepareHelper;

// An additive, idempotent catalog. Existing modes/settings are never replaced
// just because another device or orientation starts a stream.
internal static class DisplayModeCatalog
{
    internal const int Capacity = 256;
    internal static bool Merge(XElement resolutions, IEnumerable<(int Width,int Height,int Fps)> requested)
    {
        static string Key(int w,int h,int f)=>$"{w}x{h}@{f}";
        var existing=resolutions.Elements("resolution").ToList();
        int Number(XElement e,string name)=>int.TryParse(e.Element(name)?.Value,out var n)?n:0;
        var keys=existing.Select(e=>Key(Number(e,"width"),Number(e,"height"),Number(e,"refresh_rate"))).ToHashSet(StringComparer.Ordinal);
        var additions=new List<(int Width,int Height,int Fps)>();
        foreach(var mode in requested) {
            if(mode.Width<180||mode.Height<180||Math.Max(mode.Width,mode.Height)>7680||Math.Min(mode.Width,mode.Height)>4320||mode.Fps<30||mode.Fps>120||(mode.Width&1)!=0||(mode.Height&1)!=0)
                throw new InvalidDataException("Invalid device display mode.");
            if(keys.Add(Key(mode.Width,mode.Height,mode.Fps))) additions.Add(mode);
        }
        if(additions.Count==0)return false;
        // Validate the entire transaction before mutating the document.
        if(existing.Count+additions.Count>Capacity)throw new InvalidDataException("Device display catalog full.");
        foreach(var mode in additions)resolutions.Add(new XElement("resolution",new XElement("width",mode.Width),new XElement("height",mode.Height),new XElement("refresh_rate",mode.Fps)));
        return true;
    }
    internal static bool AddDeviceOrientations(XElement resolutions,int width,int height,int fps)=>
        Merge(resolutions,new[]{(width,height,fps),(height,width,fps)});
}

using System.Text;
namespace CloudgimeHostBootstrap;
internal static class HeadlessPublicRoute
{
 internal static string Build(string origin,string pcId)
 {
  if(origin.TrimEnd('/')!="https://cloudgime.my.id"||string.IsNullOrWhiteSpace(pcId)||pcId.Length>32||!pcId.All(char.IsAsciiDigit))throw new InvalidDataException("HEADLESS_PUBLIC_ROUTE_BINDING_INVALID");
  return "https://cloudgime.my.id/api/v1/application-proxy/pc/"+pcId+"/host/stream/";
 }
 internal static void Configure(string bundle)
 {
  var manifest=Path.Combine(UniversalMachineBoundary.ProvisioningRoot,"manifest.json");UniversalMachineBoundary.RequireProtected(manifest);
  var m=UniversalBundleContract.ParseObject(manifest);var desired=Build((string?)m["controlPlaneUrl"]??"",(string?)m["binding"]?["pcId"]??"");
  var file=Path.Combine(bundle,"PUBLIC_URL.txt");UniversalBundleContract.RejectReparse(file);
  if(File.Exists(file)){UniversalMachineBoundary.RequireProtected(file);if(File.ReadAllText(file).Trim()!=desired)throw new InvalidDataException("HEADLESS_PUBLIC_ROUTE_CHANGED");return;}
  var temp=file+".next";using(var output=new FileStream(temp,FileMode.CreateNew,FileAccess.Write,FileShare.None,4096,FileOptions.WriteThrough)){var b=Encoding.UTF8.GetBytes(desired+"\r\n");output.Write(b);output.Flush(true);}File.Move(temp,file,false);
 }
}

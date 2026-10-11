using System.Net.Http.Headers;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json.Nodes;
namespace CloudgimeHostBootstrap;

internal sealed class ManagedUpgradeAuthority:IDisposable
{
 private readonly string root;private readonly HttpClient http=new(new HttpClientHandler{AllowAutoRedirect=false,UseCookies=false,UseProxy=false}){Timeout=TimeSpan.FromSeconds(10)};
 internal ManagedUpgradeAuthority(string root){this.root=root;}
 internal string PcId {get;private set;}="";internal string ActivationId {get;private set;}="";
 internal void Verify(string operationId,string packageHash)
 {
  var manifest=UniversalBundleContract.ParseObject(Path.Combine(root,"manifest.json"));
  if((string?)manifest["controlPlaneUrl"] is not("https://cloudgime.my.id" or "https://cloudgime.my.id/"))throw new InvalidDataException("UPGRADE_ORIGIN_INVALID");
  PcId=(string?)manifest["binding"]?["pcId"]??throw new InvalidDataException("UPGRADE_PC_MISSING");
  var component=manifest["components"]!.AsArray().Where(x=>(string?)x?["componentType"]=="sentinel").ToArray();
  if(component.Length!=1)throw new InvalidDataException("UPGRADE_SENTINEL_AMBIGUOUS");ActivationId=(string?)component[0]?["applicationActivationId"]??throw new InvalidDataException("UPGRADE_ACTIVATION_MISSING");
  var secretPath=Path.Combine(root,"sentinel.credential.dpapi");UniversalMachineBoundary.RequireProtected(secretPath);var wrapped=File.ReadAllBytes(secretPath);byte[]? plain=null;
  try{
   plain=ProtectedData.Unprotect(wrapped,null,DataProtectionScope.LocalMachine);var data=JsonNode.Parse(plain)!.AsObject();var token=(string?)data["deviceToken"]??throw new InvalidDataException("UPGRADE_DEVICE_CREDENTIAL_MISSING");
   var requestBody=new{pcId=PcId,applicationActivationId=ActivationId,operationId,packageSha256=packageHash};
   var url=new Uri("https://cloudgime.my.id/api/v1/sentinel/managed-upgrade-state");using var request=new HttpRequestMessage(HttpMethod.Post,url);request.Headers.Authorization=new AuthenticationHeaderValue("Bearer",token);request.Content=new StringContent(System.Text.Json.JsonSerializer.Serialize(requestBody),Encoding.UTF8,"application/json");
   var began=Environment.TickCount64;using var timeout=new CancellationTokenSource(10000);using var response=http.Send(request,HttpCompletionOption.ResponseHeadersRead,timeout.Token);
   if(!response.IsSuccessStatusCode||response.RequestMessage?.RequestUri!=url)throw new InvalidDataException("UPGRADE_AUTHORITY_HTTP_"+(int)response.StatusCode);
   if(response.Content.Headers.ContentLength is >8192)throw new InvalidDataException("UPGRADE_AUTHORITY_TOO_LARGE");using var stream=response.Content.ReadAsStream(timeout.Token);using var memory=new MemoryStream();var buffer=new byte[1024];int n;
   while((n=stream.ReadAsync(buffer.AsMemory(),timeout.Token).AsTask().GetAwaiter().GetResult())>0){if(memory.Length+n>8192)throw new InvalidDataException("UPGRADE_AUTHORITY_TOO_LARGE");memory.Write(buffer,0,n);timeout.Token.ThrowIfCancellationRequested();}
   var r=JsonNode.Parse(memory.ToArray())!.AsObject();
   var observed=DateTimeOffset.Parse((string)r["observedAt"]!,System.Globalization.CultureInfo.InvariantCulture);var until=DateTimeOffset.Parse((string)r["validUntil"]!,System.Globalization.CultureInfo.InvariantCulture);
   if((bool?)r["ok"]!=true||(int?)r["schemaVersion"]!=1||(string?)r["pcId"]!=PcId||(string?)r["applicationActivationId"]!=ActivationId||(string?)r["operationId"]!=operationId||(string?)r["packageSha256"]!=packageHash||(bool?)r["maintenance"]!=true||r["activeClaim"]!=null||(int?)r["activeRentals"]!=0||(int?)r["cleanupHolds"]!=0||(bool?)r["billingMutation"]!=false||until<=observed||until-observed>TimeSpan.FromSeconds(10)||Environment.TickCount64-began>=(until-observed).TotalMilliseconds||Math.Abs((DateTimeOffset.UtcNow-observed).TotalSeconds)>30)throw new InvalidDataException("UPGRADE_AUTHORITY_INVALID_OR_STALE");
  }finally{CryptographicOperations.ZeroMemory(wrapped);if(plain!=null)CryptographicOperations.ZeroMemory(plain);}
 }
 public void Dispose()=>http.Dispose();
}

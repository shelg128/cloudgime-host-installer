using System.Text.Json.Nodes;

namespace CloudgimeHostBootstrap;

internal static class UniversalTransportPolicy
{
    // Remove relay configuration without changing the assigned route, port, account,
    // ticket enforcement, or any other machine-specific configuration.
    internal static JsonObject DirectOnly(JsonObject current)
    {
        var result = (JsonObject)current.DeepClone();
        var webrtc = result["webrtc"] as JsonObject ?? throw new InvalidDataException("WEBRTC_CONFIG_REQUIRED");
        var servers = webrtc["ice_servers"] as JsonArray ?? throw new InvalidDataException("ICE_CONFIG_REQUIRED");
        var direct = new JsonArray();
        foreach (var entry in servers)
        {
            if (entry is not JsonObject server || server["urls"] is not JsonArray urls)
                throw new InvalidDataException("ICE_CONFIG_INVALID");
            var stun = new JsonArray();
            foreach (var value in urls)
            {
                if (value is not JsonValue scalar || !scalar.TryGetValue<string>(out var url))
                    throw new InvalidDataException("ICE_URL_INVALID");
                if (url.StartsWith("stun:", StringComparison.OrdinalIgnoreCase)) stun.Add(url);
                else if (!url.StartsWith("turn:", StringComparison.OrdinalIgnoreCase) && !url.StartsWith("turns:", StringComparison.OrdinalIgnoreCase))
                    throw new InvalidDataException("ICE_SCHEME_INVALID");
            }
            if (stun.Count > 0) direct.Add(new JsonObject { ["urls"] = stun });
        }
        if (direct.Count == 0) direct.Add(new JsonObject { ["urls"] = new JsonArray("stun:stun.cloudflare.com:3478") });
        webrtc["ice_servers"] = direct;
        return result;
    }
}

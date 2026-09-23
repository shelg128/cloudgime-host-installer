using System.Text.Json.Nodes;
using CloudgimeHostBootstrap;

internal static class TransportPolicyTests
{
    internal static int Run()
    {
        var original=JsonNode.Parse("""
        {"web_server":{"bind_address":"127.0.0.1:18021","url_path_prefix":"/bound-path","rental_stream_ticket_required":true},"webrtc":{"port_range":{"min":41000,"max":41049},"ice_servers":[{"urls":["stun:stun.example:3478","turn:relay.example:3478"],"username":"old","credential":"fixture-only"},{"urls":["turns:relay.example:443"]}]},"unrelated":{"enabled":true}}
        """)!.AsObject();
        var before=original.ToJsonString();var direct=UniversalTransportPolicy.DirectOnly(original);
        void Check(bool v){if(!v)throw new Exception("DIRECT_MIGRATION_CONTRACT_FAILED");}
        Check(original.ToJsonString()==before);
        Check(JsonNode.DeepEquals(direct["web_server"],original["web_server"]));
        Check(JsonNode.DeepEquals(direct["webrtc"]!["port_range"],original["webrtc"]!["port_range"]));
        Check(JsonNode.DeepEquals(direct["unrelated"],original["unrelated"]));
        Check(direct["webrtc"]!["ice_servers"]!.AsArray().Count==1);
        Check(direct["webrtc"]!["ice_servers"]![0]!["urls"]!.AsArray().Count==1);
        Check(!direct["webrtc"]!["ice_servers"]!.ToJsonString().Contains("credential"));
        Check(JsonNode.DeepEquals(direct,UniversalTransportPolicy.DirectOnly(direct)));
        return 8;
    }
}

using System.Text.Json;

namespace DisplayPrepareHelper;

// Read-only deployment/build identity. Does not probe hardware or modify profile,
// settings, license, display, sessions or filesystem.
internal static class EncoderRuntimeContract
{
    internal const string Id = "cloudgime-encoder-stability-v1";
    internal static string Json() => JsonSerializer.Serialize(new
    {
        schema = 1, contract = Id, nvencLogPolicyVersion = NvencLogEvidence.PolicyVersion,
        orderedH264Recovery = true, boundedLogBytes = NvencLogEvidence.MaxCharacters,
        modernAndLegacyTimestamps = true, stalePolicyCacheReprobed = true,
        freshProbeRequired = true, unresolvedFailuresPreserved = true,
        capabilityAwareSelection = true, globalFixedEncoder = false,
        globalFixedVideoProfile = false, routeOrBillingChanges = false,
        physicalQualityAccepted = false
    });
}

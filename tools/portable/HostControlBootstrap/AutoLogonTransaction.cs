namespace CloudgimeHostBootstrap;

// OS-independent transaction. The snapshot is sensitive; never log or serialize it.
internal sealed record AutoLogonSnapshot(string? Enabled, string? User, string? Domain,
    string? OwnerSid, string? LsaSecret, bool PlaintextValuePresent);
internal interface IAutoLogonStore
{
    AutoLogonSnapshot Read();
    void SetEnabled(string? value);
    void SetSecret(string? value);
    void SetIdentity(string? user, string? domain, string? ownerSid);
}
internal static class AutoLogonTransaction
{
    internal static void Configure(IAutoLogonStore store, string user, string domain, string sid, string password)
    {
        if (string.IsNullOrWhiteSpace(user) || string.IsNullOrWhiteSpace(domain)
            || !sid.StartsWith("S-1-5-21-", StringComparison.Ordinal) || string.IsNullOrEmpty(password))
            throw new InvalidOperationException("AUTOLOGON_INPUT_INVALID");
        var before = store.Read();
        RequireOwnership(before, user, domain, sid);
        try
        {
            store.SetEnabled("0");
            store.SetSecret(password);
            store.SetIdentity(user, domain, sid);
            store.SetEnabled("1"); // Commit last; a partial setup never enables autologon.
            var after = store.Read();
            if (after.Enabled != "1" || after.User != user || after.Domain != domain
                || after.OwnerSid != sid || after.LsaSecret != password || after.PlaintextValuePresent)
                throw new InvalidOperationException("AUTOLOGON_VERIFY_FAILED");
        }
        catch (Exception original)
        {
            try { Restore(store, before); }
            catch (Exception rollback) { throw new InvalidOperationException("AUTOLOGON_ROLLBACK_FAILED: administrator recovery required; automatic login remains disabled.", new AggregateException(original, rollback)); }
            throw;
        }
    }

    internal static void Disable(IAutoLogonStore store)
    {
        var before = store.Read();
        if (string.IsNullOrEmpty(before.OwnerSid)) throw new InvalidOperationException("AUTOLOGON_NOT_OWNED");
        if (before.PlaintextValuePresent) throw new InvalidOperationException("AUTOLOGON_EXTERNALLY_MODIFIED");
        try
        {
            store.SetEnabled("0");
            store.SetSecret(null);
            store.SetIdentity(null, null, null);
            var after = store.Read();
            if (after.Enabled != "0" || after.LsaSecret != null || after.OwnerSid != null
                || after.User != null || after.Domain != null)
                throw new InvalidOperationException("AUTOLOGON_DISABLE_VERIFY_FAILED");
        }
        catch (Exception original)
        {
            try { Restore(store, before); }
            catch (Exception rollback) { throw new InvalidOperationException("AUTOLOGON_ROLLBACK_FAILED: administrator recovery required; automatic login remains disabled.", new AggregateException(original, rollback)); }
            throw;
        }
    }

    private static void RequireOwnership(AutoLogonSnapshot s, string user, string domain, string sid)
    {
        if (s.PlaintextValuePresent) throw new InvalidOperationException("EXISTING_PLAINTEXT_AUTOLOGON: remove or migrate through the owning administrator first.");
        if (s.OwnerSid is not null)
        {
            if (s.OwnerSid != sid || !string.Equals(s.User, user, StringComparison.OrdinalIgnoreCase)
                || !string.Equals(s.Domain, domain, StringComparison.OrdinalIgnoreCase))
                throw new InvalidOperationException("AUTOLOGON_OWNER_MISMATCH");
        }
        else if (s.Enabled == "1" || s.LsaSecret is not null || !string.IsNullOrEmpty(s.User) || !string.IsNullOrEmpty(s.Domain))
            throw new InvalidOperationException("EXISTING_UNMANAGED_AUTOLOGON: existing Windows configuration is preserved.");
    }

    private static void Restore(IAutoLogonStore store, AutoLogonSnapshot before)
    {
        try
        {
            store.SetEnabled("0");
            store.SetSecret(before.LsaSecret);
            store.SetIdentity(before.User, before.Domain, before.OwnerSid);
            store.SetEnabled(before.Enabled);
        }
        catch
        {
            // Fail closed if restoring the old configuration itself fails.
            try { store.SetEnabled("0"); } catch { }
            throw new InvalidOperationException("AUTOLOGON_ROLLBACK_FAILED: administrator recovery required; automatic login must remain disabled.");
        }
    }
}

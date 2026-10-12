using System.Text.Json.Nodes;
using CloudgimeHostBootstrap;

static void Require(bool value, string code) { if (!value) throw new InvalidOperationException(code); }
static void Throws(Action action, string contains) { try { action(); } catch (Exception e) when (e.Message.Contains(contains, StringComparison.Ordinal)) { return; } throw new InvalidOperationException("EXPECTED_" + contains); }

if(HeadlessJournalTests.Worker(args))return;
if(HeadlessProcessTests.Worker(args))return;
if(ManagedCrashProcessTests.Worker(args))return;
ManagedCrashProcessTests.Run();

var activated = new JsonObject { ["ActivationState"]="activated", ["HostId"]="cgslot-pc02", ["ActivationRecordId"]="cgslot-pc02", ["CredentialRef"]="host_app.credential.dpapi" };
Require(UniversalInstallPolicy.IsActivated(activated), "ACTIVATED_NOT_DETECTED");
Throws(() => UniversalInstallPolicy.CheckReset(true, true, false), "EXISTING_NODE_IDENTITY");
Throws(() => UniversalInstallPolicy.CheckReset(true, false, true), "EXISTING_NODE_IDENTITY");
UniversalInstallPolicy.CheckReset(true, false, false);
UniversalInstallPolicy.CheckReset(false, true, true);
Require(UniversalInstallPolicy.AcceptActiveDesktop("operator","operator",null,null),"LEGACY_OPERATOR_DESKTOP_REJECTED");
Require(UniversalInstallPolicy.AcceptActiveDesktop("renter","renter","renter","operator"),"DEDICATED_RENTER_DESKTOP_REJECTED");
Require(UniversalInstallPolicy.AcceptActiveDesktop("operator","operator","renter","operator"),"SHARED_OPERATOR_DESKTOP_REJECTED");
Require(!UniversalInstallPolicy.AcceptActiveDesktop("operator","renter","renter","operator"),"ACTIVE_SID_MISMATCH_ACCEPTED");
Require(!UniversalInstallPolicy.AcceptActiveDesktop("other","other","renter","operator"),"UNBOUND_DESKTOP_ACCEPTED");
Require(!UniversalInstallPolicy.AcceptActiveDesktop("renter","renter","renter","renter"),"SAME_ROLE_SID_ACCEPTED");
Require(UniversalInstallPolicy.Preserve("provisioning/host_app.credential.dpapi"), "PROVISIONING_NOT_PRESERVED");
Require(UniversalInstallPolicy.Preserve("keeper-tunnel/data/cloudrental.env"), "TUNNEL_NOT_PRESERVED");
UniversalBundleContract.RequireWindowsExecutable(Environment.ProcessPath ?? throw new InvalidOperationException("TEST_EXE_MISSING"));
var fakeExecutable = Path.Combine(Path.GetTempPath(), "cg-fake-exe-" + Guid.NewGuid().ToString("N"));
File.WriteAllText(fakeExecutable, "INERT_TEST_FIXTURE_NOT_EXECUTABLE");
try { Throws(() => UniversalBundleContract.RequireWindowsExecutable(fakeExecutable), "NON_EXECUTABLE_PAYLOAD"); }
finally { File.Delete(fakeExecutable); }
Require(!UniversalInstallPolicy.Preserve("moonlight/web-server.exe"), "BINARY_WRONGLY_PRESERVED");
Throws(() => UniversalInstallPolicy.RejectCommandLineSecrets(["install","--auto-logon-password","secret"]), "PLAINTEXT_ARGUMENT_REJECTED");

var store = new FakeStore(new(null,null,null,null,null,false));
AutoLogonTransaction.Configure(store,"admin","PC","S-1-5-21-1-2-3-1001","secret-value");
Require(store.Value == new AutoLogonSnapshot("1","admin","PC","S-1-5-21-1-2-3-1001","secret-value",false), "CONFIGURE_VERIFY");
AutoLogonTransaction.Disable(store);
Require(store.Value == new AutoLogonSnapshot("0",null,null,null,null,false), "DISABLE_VERIFY");

var before = new AutoLogonSnapshot("0",null,null,null,null,false);
var failing = new FakeStore(before) { FailAt = 3 };
Throws(() => AutoLogonTransaction.Configure(failing,"admin","PC","S-1-5-21-1-2-3-1001","secret"), "INJECTED");
Require(failing.Value == before, "ROLLBACK_VERIFY");
Throws(() => AutoLogonTransaction.Configure(new FakeStore(new("1","other","PC",null,null,false)),"admin","PC","S-1-5-21-1-2-3-1001","secret"), "EXISTING_UNMANAGED_AUTOLOGON");
var rollbackFailure = new FakeStore(before) { FailAt = 3, AdditionalFailAt = 5 };
Throws(() => AutoLogonTransaction.Configure(rollbackFailure,"admin","PC","S-1-5-21-1-2-3-1001","secret"), "AUTOLOGON_ROLLBACK_FAILED");
Require(rollbackFailure.Value.Enabled == "0", "ROLLBACK_FAILURE_NOT_DISABLED");
HeadlessBundleTests.Run();
HeadlessJournalTests.Run();
HeadlessProcessTests.Run();
HeadlessSetupTests.Run();
ManagedJournalMigrationTests.Run();
ManagedTransactionTests.Run();
ManagedUpgradeGuardTests.Run();
var installableEncoderTests = InstallableEncoderTests.Run();
var encoderTests = EncoderContractTests.Run();
var bundleTests = BundleTests.Run();
var journalTests = IdleJournalTests.Run();
var matchedUpgradeTests = MatchedUpgradeTests.Run();
Console.WriteLine($"MATCHED_UPGRADE_FILE_ONLY_ASSERTIONS={matchedUpgradeTests}; PHYSICAL_INSTALLER_QUALIFIED=false");
var fileTests = FileTransactionTests.Run();
var transportTests = TransportPolicyTests.Run();
Console.WriteLine($"ENCODER_INSTALLABLE_BUNDLE_ASSERTIONS={installableEncoderTests}");
Console.WriteLine($"ENCODER_INSTALL_CONTRACT_ASSERTIONS={encoderTests}");
Console.WriteLine($"UNIVERSAL_INSTALL_POLICY_TESTS_OK; BUNDLE_ASSERTIONS={bundleTests}; IDLE_JOURNAL_ASSERTIONS={journalTests}; FILE_TRANSACTION_ASSERTIONS={fileTests}; TRANSPORT_ASSERTIONS={transportTests}");

sealed class FakeStore(AutoLogonSnapshot value) : IAutoLogonStore
{
    internal AutoLogonSnapshot Value { get; private set; } = value; internal int FailAt { get; init; } = -1; internal int AdditionalFailAt { get; init; } = -1; int calls;
    void MaybeFail(){ calls++; if(calls==FailAt || calls==AdditionalFailAt) throw new InvalidOperationException("INJECTED"); }
    public AutoLogonSnapshot Read()=>Value;
    public void SetEnabled(string? v){MaybeFail();Value=Value with{Enabled=v};}
    public void SetSecret(string? v){MaybeFail();Value=Value with{LsaSecret=v,PlaintextValuePresent=false};}
    public void SetIdentity(string? u,string? d,string? s){MaybeFail();Value=Value with{User=u,Domain=d,OwnerSid=s};}
}

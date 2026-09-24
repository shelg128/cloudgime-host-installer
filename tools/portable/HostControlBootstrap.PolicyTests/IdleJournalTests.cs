using System.Security.Cryptography;
using System.Text;
using CloudgimeHostBootstrap;

internal static class IdleJournalTests
{
    internal static int Run()
    {
        var key=RandomNumberGenerator.GetBytes(32);
        byte[] Encrypt(string text)
        {
            var plain=Encoding.UTF8.GetBytes(text);
            var result=new byte[plain.Length+31];"v10"u8.CopyTo(result);
            RandomNumberGenerator.Fill(result.AsSpan(3,12));
            using var aes=new AesGcm(key,16);
            aes.Encrypt(result.AsSpan(3,12),plain,result.AsSpan(15,plain.Length),result.AsSpan(15+plain.Length,16));
            CryptographicOperations.ZeroMemory(plain);return result;
        }
        void Check(bool v,string code){if(!v)throw new Exception(code);}
        try
        {
            var idle=Encrypt("{\"version\":1,\"phase\":\"IDLE\"}");
            Check(ElectronIdleJournal.IsIdle(idle,key),"VALID_IDLE_REJECTED");
            foreach(var text in new[]{"{\"version\":1,\"phase\":\"COMMITTED\"}","{\"version\":1,\"phase\":\"ABORTED\"}","{\"version\":1,\"phase\":\"IDLE\",\"sessionId\":\"extra\"}","{\"version\":1,\"phase\":\"IDLE\",\"phase\":\"IDLE\"}","{\"version\":2,\"phase\":\"IDLE\"}","[]","garbage"})
                Check(!ElectronIdleJournal.IsIdle(Encrypt(text),key),"INVALID_IDLE_ACCEPTED");
            var corrupt=(byte[])idle.Clone();corrupt[20]^=1;Check(!ElectronIdleJournal.IsIdle(corrupt,key),"CORRUPT_ACCEPTED");
            corrupt[0]=(byte)'x';Check(!ElectronIdleJournal.IsIdle(corrupt,key),"FORMAT_ACCEPTED");
            Check(!ElectronIdleJournal.IsIdle(idle,new byte[32]),"WRONG_KEY_ACCEPTED");
            Check(!ElectronIdleJournal.IsIdle([],key),"EMPTY_ACCEPTED");
            var done=Encrypt("{\"stage\":\"DONE\",\"command\":{\"operationId\":\"fixture\"}}");
            Check(ElectronIdleJournal.IsCleanupDone(done,key),"VALID_DONE_REJECTED");
            foreach(var stage in new[]{"STARTED","LOCAL_PROOF","RECEIPT_ACCEPTED","LOCAL_IDLE"})
                Check(!ElectronIdleJournal.IsCleanupDone(Encrypt("{\"stage\":\""+stage+"\"}"),key),"UNFINISHED_CLEANUP_ACCEPTED");
            var badDone=(byte[])done.Clone();badDone[^1]^=1;Check(!ElectronIdleJournal.IsCleanupDone(badDone,key),"CORRUPT_DONE_ACCEPTED");
            return 18;
        }
        finally {CryptographicOperations.ZeroMemory(key);}
    }
}

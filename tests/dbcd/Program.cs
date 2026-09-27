using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using DBCD.Providers;

if (args.Length != 4) throw new ArgumentException("Expected fixture directory, table, build, output JSON");
var db = new DBCD.DBCD(new FilesystemDBCProvider(args[0]), new FilesystemDBDProvider(args[0]));
var table = db.Load(args[1], args[2]);
var ids = table.Keys.Order().ToArray();
var idText = string.Join("\n", ids) + "\n";
var rows = ids.Take(200).Select(id => table.AvailableColumns.ToDictionary(name => name, name => table[id][name])).ToArray();
var result = new {
    sourceCommit = "e732093f8864240fc5884bd1bba6b02f3dfc0d56",
    count = ids.Length,
    idDigest = Convert.ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes(idText))).ToLowerInvariant(),
    encryptedIDs = table.GetEncryptedIDs().ToDictionary(pair => pair.Key.ToString("x16"), pair => pair.Value.Order().ToArray()),
    rows
};
File.WriteAllText(args[3], JsonSerializer.Serialize(result));

using System.Collections;
using System.Globalization;
using System.Reflection;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using DBCD.Providers;

if (args.Length < 4 || args.Length > 6) throw new ArgumentException("Expected fixture directory, table, build, output JSON, optional comma-separated row IDs (or -), optional hotfix cache");
var db = new DBCD.DBCD(new FilesystemDBCProvider(args[0]), new FilesystemDBDProvider(args[0]));
var table = db.Load(args[1], args[2]);
if (args.Length == 6) table.ApplyingHotfixes(new DBCD.IO.HotfixReader(args[5]));
var ids = table.Keys.Order().ToArray();
var idText = string.Join("\n", ids) + "\n";
if (ids.Length == 0) idText = "";
var sampleIDs = args.Length >= 5 && args[4] != "-" ? args[4].Split(',').Select(int.Parse).Distinct().Order().ToArray() : ids.Take(200).ToArray();
if (sampleIDs.Length > 200) throw new ArgumentException("At most 200 sampled IDs");
var columns = table.AvailableColumns.Order(StringComparer.Ordinal).ToArray();
var rows = sampleIDs.Select(id => columns.ToDictionary(name => name, name => table[id][name])).ToArray();
using var all = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
using var block = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
var blocks = new List<string>();
for (var index = 0; index < ids.Length; index++) {
    var canonical = new StringBuilder();
    Append(canonical, ids[index]);
    foreach (var name in columns) { Append(canonical, name); Append(canonical, table[ids[index]][name]); }
    var bytes = Encoding.UTF8.GetBytes(canonical.ToString());
    all.AppendData(bytes); block.AppendData(bytes);
    if ((index+1)%256 == 0 || index+1 == ids.Length) blocks.Add(Convert.ToHexString(block.GetHashAndReset()).ToLowerInvariant());
}
var sourceCommit = Assembly.GetExecutingAssembly().GetCustomAttributes<AssemblyMetadataAttribute>().Single(a => a.Key == "DBCDSourceCommit").Value;
var result = new {
    sourceCommit,
    build = args[2],
    db2SHA256 = HashFile(Path.Combine(args[0], args[1]+".db2")),
    dbdSHA256 = HashFile(Path.Combine(args[0], args[1]+".dbd")),
    hotfixSHA256 = args.Length == 6 ? HashFile(args[5]) : "",
    count = ids.Length,
    idDigest = Convert.ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes(idText))).ToLowerInvariant(),
    fieldDigest = Convert.ToHexString(all.GetHashAndReset()).ToLowerInvariant(),
    fieldBlocks = blocks,
    encryptedIDs = table.GetEncryptedIDs().ToDictionary(pair => pair.Key.ToString("x16"), pair => pair.Value.Order().ToArray()),
    rows
};
File.WriteAllText(args[3], JsonSerializer.Serialize(result));

static string HashFile(string path) => Convert.ToHexString(SHA256.HashData(File.ReadAllBytes(path))).ToLowerInvariant();
static void Append(StringBuilder output, object? value) {
    switch(value) {
        case null: output.Append("z;"); break;
        case string text: output.Append('s').Append(Encoding.UTF8.GetByteCount(text).ToString(CultureInfo.InvariantCulture)).Append(':').Append(text); break;
        case float f: output.Append('f').Append(BitConverter.DoubleToUInt64Bits((double)f).ToString("x16")); break;
        case double d: output.Append('f').Append(BitConverter.DoubleToUInt64Bits(d).ToString("x16")); break;
        case IEnumerable array:
            var values = array.Cast<object?>().ToArray();
            output.Append('a').Append(values.Length.ToString(CultureInfo.InvariantCulture)).Append(':');
            foreach(var item in values) Append(output,item);
            break;
        default: output.Append('n').Append(Convert.ToString(value,CultureInfo.InvariantCulture)).Append(';'); break;
    }
}

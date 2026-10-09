using System.Text.Json;
using System.Text.Json.Serialization;

namespace System.Web.Script.Serialization
{
    // Minimal stand-in for the .NET Framework JavaScriptSerializer so the core tests can run on modern .NET (macOS/Linux).
    // Only the surface PreferenceStore uses: MaxJsonLength, Deserialize<T>(string), Serialize(object). Not part of the app.
    public class JavaScriptSerializer
    {
        static readonly JsonSerializerOptions Options = new JsonSerializerOptions
        {
            IncludeFields = true, PropertyNameCaseInsensitive = true,
            NumberHandling = JsonNumberHandling.AllowNamedFloatingPointLiterals
        };
        public int MaxJsonLength { get; set; }
        public T Deserialize<T>(string input)
        {
            if (MaxJsonLength > 0 && input != null && input.Length > MaxJsonLength) throw new ArgumentException("MaxJsonLength exceeded.");
            return JsonSerializer.Deserialize<T>(input, Options);
        }
        public string Serialize(object value)
        {
            return JsonSerializer.Serialize(value, value == null ? typeof(object) : value.GetType(), Options);
        }
    }
}

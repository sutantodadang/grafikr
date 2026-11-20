package dev.grafikr;

import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import java.math.BigDecimal;

public record CollectionRequest(String revision, String request, long start, int count) {
    static CollectionRequest from(JsonObject event) throws BridgeException {
        JsonElement payloadElement = event.get("payload");
        if (payloadElement == null || !payloadElement.isJsonObject()) throw invalid();
        JsonObject payload = payloadElement.getAsJsonObject();
        JsonElement revision = payload.get("revision"), request = payload.get("request");
        JsonElement start = payload.get("start"), count = payload.get("count");
        if (revision == null || !revision.isJsonPrimitive() || !revision.getAsJsonPrimitive().isString()
                || request == null || !request.isJsonPrimitive() || !request.getAsJsonPrimitive().isString()
                || start == null || !start.isJsonPrimitive() || !start.getAsJsonPrimitive().isNumber()
                || count == null || !count.isJsonPrimitive() || !count.getAsJsonPrimitive().isNumber()) throw invalid();
        try {
            long startValue = new BigDecimal(start.getAsString()).longValueExact();
            int countValue = Math.toIntExact(new BigDecimal(count.getAsString()).longValueExact());
            if (startValue < 0 || countValue < 0 || countValue > 512) throw invalid();
            return new CollectionRequest(revision.getAsString(), request.getAsString(), startValue, countValue);
        } catch (NumberFormatException | ArithmeticException error) {
            throw invalid();
        }
    }

    private static BridgeException invalid() {
        return new BridgeException("invalid_frame", "Collection request fields are invalid");
    }
}

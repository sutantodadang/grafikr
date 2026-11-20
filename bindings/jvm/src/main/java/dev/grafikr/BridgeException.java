package dev.grafikr;

public final class BridgeException extends RuntimeException {
    private final String code;
    public BridgeException(String code, String message) { super(message); this.code=code; }
    public String code() { return code; }
}

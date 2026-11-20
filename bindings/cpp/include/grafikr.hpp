#ifndef GRAFIKR_HPP
#define GRAFIKR_HPP
#include "grafikr_client.h"
#include <cstdint>
#include <chrono>
#include <cmath>
#include <condition_variable>
#include <deque>
#include <exception>
#include <functional>
#include <limits>
#include <memory>
#include <mutex>
#include <optional>
#include <stdexcept>
#include <string_view>
#include <string>
#include <utility>
#include <vector>

namespace grafikr {
class Error : public std::runtime_error { public: using std::runtime_error::runtime_error; };
class BridgeError : public Error {
public:
    BridgeError(std::string code, std::string message, std::string field, std::string handle)
        : Error(std::move(message)), code_(std::move(code)), field_(std::move(field)), handle_(std::move(handle)) {}
    const std::string& code() const noexcept { return code_; }
    const std::string& field() const noexcept { return field_; }
    const std::string& handle() const noexcept { return handle_; }
private:
    std::string code_;
    std::string field_;
    std::string handle_;
};

class Client;

class Value {
public:
    Value(const Value&) = delete;
    Value& operator=(const Value&) = delete;
    Value(Value&& other) noexcept : value_(std::exchange(other.value_, nullptr)) {}
    Value& operator=(Value&& other) noexcept {
        if (this != &other) {
            gf_value_free(value_);
            value_ = std::exchange(other.value_, nullptr);
        }
        return *this;
    }
    ~Value() noexcept { gf_value_free(value_); }

    static Value object() { return Value(require(gf_value_object())); }
    static Value array() { return Value(require(gf_value_array())); }
    static Value boolean(bool value) { return Value(require(gf_value_bool(value))); }
    static Value i64(std::int64_t value) { return Value(require(gf_value_i64(value))); }
    static Value number(double value) { return Value(require(gf_value_f64(value))); }
    static Value string(std::string_view value) {
        auto text = terminated(value);
        return Value(require(gf_value_string(text.c_str())));
    }

    void set(std::string_view key, Value&& child) {
        auto name = terminated(key);
        auto* pointer = std::exchange(child.value_, nullptr);
        auto status = gf_value_object_set(value_, name.c_str(), pointer);
        if (status != GRAFIKR_OK) child.value_ = pointer;
        check(status);
    }
    void push(Value&& child) {
        auto* pointer = std::exchange(child.value_, nullptr);
        auto status = gf_value_array_append(value_, pointer);
        if (status != GRAFIKR_OK) child.value_ = pointer;
        check(status);
    }
    Value get(std::string_view key) const {
        auto name = terminated(key);
        const gf_value* child = nullptr;
        check(gf_value_object_get(value_, name.c_str(), &child));
        return Value(require(gf_value_clone(child)));
    }
    Value at(std::size_t index) const {
        const gf_value* child = nullptr;
        check(gf_value_array_get(value_, index, &child));
        return Value(require(gf_value_clone(child)));
    }
    std::size_t size() const { return gf_value_array_size(value_); }
    std::string to_json() const {
        std::size_t required = 0;
        auto status = gf_value_write_json(value_, nullptr, 0, &required);
        if (status != GRAFIKR_BUFFER_TOO_SMALL && status != GRAFIKR_OK) check(status);
        std::string buffer(required, '\0');
        check(gf_value_write_json(value_, buffer.data(), buffer.size(), &required));
        buffer.pop_back();
        return buffer;
    }
    bool as_bool() const {
        bool result = false;
        check(gf_value_get_bool(value_, &result));
        return result;
    }
    std::int64_t as_i64() const {
        std::int64_t result = 0;
        check(gf_value_get_i64(value_, &result));
        return result;
    }
    double as_f64() const {
        double result = 0;
        check(gf_value_get_f64(value_, &result));
        return result;
    }
    std::string as_string() const {
        const char* result = nullptr;
        check(gf_value_get_string(value_, &result));
        return result;
    }

private:
    friend class Client;
    friend class Subscription;
    explicit Value(gf_value* value) : value_(value) {}
    gf_value* value_;

    static gf_value* require(gf_value* value) {
        if (!value) throw Error("Grafikr value allocation or validation failed");
        return value;
    }
    static void check(gf_status status) {
        if (status != GRAFIKR_OK) throw Error("Grafikr value operation failed: " + std::to_string(status));
    }
    static std::string terminated(std::string_view value) {
        if (value.find('\0') != std::string_view::npos) throw Error("Grafikr strings cannot contain NUL bytes");
        return std::string(value);
    }
};

namespace detail {
struct CallbackErrors {
    using Handler = std::function<void(std::exception_ptr, const Value&)>;
    std::mutex mutex;
    Handler handler;

    void set(Handler value) {
        std::lock_guard<std::mutex> lock(mutex);
        handler = std::move(value);
    }
    void report(std::exception_ptr error, const Value& event) noexcept {
        Handler current;
        try {
            { std::lock_guard<std::mutex> lock(mutex); current = handler; }
            if (current) current(std::move(error), event);
        } catch (...) {}
    }
};
}

class Subscription {
    struct CallbackContext : std::enable_shared_from_this<CallbackContext> {
        std::function<void(const Value&)> callback;
        std::shared_ptr<detail::CallbackErrors> errors;
    };
public:
    Subscription(const Subscription&) = delete;
    Subscription& operator=(const Subscription&) = delete;
    Subscription(Subscription&& other) noexcept
        : subscription_(std::exchange(other.subscription_, nullptr)), context_(std::move(other.context_)) {}
    Subscription& operator=(Subscription&& other) noexcept {
        if (this != &other) {
            reset();
            subscription_ = std::exchange(other.subscription_, nullptr);
            context_ = std::move(other.context_);
        }
        return *this;
    }
    ~Subscription() noexcept { reset(); }
    void close() noexcept { reset(); }
private:
    friend class Client;
    Subscription(gf_subscription* subscription, std::shared_ptr<CallbackContext> context)
        : subscription_(subscription), context_(std::move(context)) {}
    gf_subscription* subscription_{};
    std::shared_ptr<CallbackContext> context_;
    static void GRAFIKR_CALL dispatch(gf_client*, const gf_value* event, void* userdata) noexcept {
        auto context = static_cast<CallbackContext*>(userdata)->shared_from_this();
        try {
            Value copy(Value::require(gf_value_clone(event)));
            try { context->callback(copy); }
            catch (...) { context->errors->report(std::current_exception(), copy); }
        } catch (...) {}
    }
    void reset() noexcept {
        if (subscription_) (void)gf_off(std::exchange(subscription_, nullptr));
        context_.reset();
    }
};

class Client {
public:
    using ErrorHandler = detail::CallbackErrors::Handler;
    void on_error(ErrorHandler handler) { errors_->set(std::move(handler)); }

    Value request(std::string_view op, const Value& args) {
        auto operation = terminated(op);
        gf_value* result = nullptr;
        auto status = gf_request(client_, operation.c_str(), args.value_, &result);
        if (status == GRAFIKR_BRIDGE_ERROR) {
            auto code = text(result, "code");
            auto message = text(result, "message");
            auto field = text(result, "field");
            auto handle = text(result, "handle");
            gf_value_free(result);
            throw BridgeError(std::move(code), std::move(message), std::move(field), std::move(handle));
        }
        if (status != GRAFIKR_OK) {
            gf_value_free(result);
            throw Error("Grafikr request failed: " + std::to_string(status));
        }
        return Value(Value::require(result));
    }
    Subscription on(std::uint64_t node, std::string_view event, std::function<void(const Value&)> callback) {
        if (!callback) throw Error("Grafikr subscription callback is empty");
        auto name = terminated(event);
        auto context = std::make_shared<Subscription::CallbackContext>();
        context->callback = std::move(callback);
        context->errors = errors_;
        gf_subscription* subscription = nullptr;
        auto status = gf_on(client_, node, name.c_str(), Subscription::dispatch, context.get(), &subscription);
        if (status != GRAFIKR_OK) throw Error("Grafikr subscription failed: " + std::to_string(status));
        return Subscription(subscription, std::move(context));
    }
private:
    friend class Application;
    explicit Client(gf_client* client) : client_(client), errors_(std::make_shared<detail::CallbackErrors>()) {}
    gf_client* client_;
    std::shared_ptr<detail::CallbackErrors> errors_;

    static std::string terminated(std::string_view value) {
        if (value.find('\0') != std::string_view::npos) throw Error("Grafikr strings cannot contain NUL bytes");
        return std::string(value);
    }
    static std::string text(const gf_value* object, const char* key) {
        const gf_value* value = nullptr;
        const char* result = nullptr;
        if (object && gf_value_object_get(object, key, &value) == GRAFIKR_OK &&
            gf_value_get_string(value, &result) == GRAFIKR_OK)
            return result;
        return {};
    }
};

struct CollectionMetadata {
    std::uint64_t row_count{};
    double row_height{};
    std::vector<std::string> columns;

    Value to_value() const {
        if (row_count > static_cast<std::uint64_t>(std::numeric_limits<std::uint32_t>::max()) + 1 ||
            !std::isfinite(row_height) || row_height <= 0 || columns.empty() || columns.size() > 256)
            throw BridgeError("invalid_value", "Collection metadata is invalid", "", "");
        auto value = Value::object();
        value.set("row_count", Value::i64(row_count));
        value.set("row_height", Value::number(row_height));
        auto names = Value::array();
        for (const auto& column : columns) names.push(Value::string(column));
        value.set("columns", std::move(names));
        return value;
    }
};

struct CollectionRequest {
    std::string revision;
    std::string request;
    std::uint32_t start{};
    std::uint32_t count{};

    static CollectionRequest from_event(const Value& event) {
        try {
            auto payload = event.get("payload");
            auto revision = payload.get("revision").as_string();
            auto request = payload.get("request").as_string();
            auto start = payload.get("start").as_i64();
            auto count = payload.get("count").as_i64();
            if (start < 0 || start > std::numeric_limits<std::uint32_t>::max() || count < 0 || count > 512)
                throw BridgeError("invalid_frame", "Collection request range is invalid", "", "");
            return {std::move(revision), std::move(request), static_cast<std::uint32_t>(start), static_cast<std::uint32_t>(count)};
        } catch (const BridgeError&) { throw; }
        catch (const std::exception&) { throw BridgeError("invalid_frame", "Collection request fields are invalid", "", ""); }
    }
};

class CollectionProvider {
    struct State {
        std::mutex mutex;
        std::condition_variable ready;
        std::deque<CollectionRequest> requests;
        std::exception_ptr failure;
        bool closed{};
        bool reader_waiting{};
    };
public:
    CollectionProvider(const CollectionProvider&) = delete;
    CollectionProvider& operator=(const CollectionProvider&) = delete;
    CollectionProvider(CollectionProvider&& other) noexcept
        : client_(std::exchange(other.client_, nullptr)), node_(other.node_), subscription_(std::move(other.subscription_)), state_(std::move(other.state_)) {}
    CollectionProvider& operator=(CollectionProvider&& other) noexcept {
        if (this != &other) {
            close(); client_ = std::exchange(other.client_, nullptr); node_ = other.node_;
            subscription_ = std::move(other.subscription_); state_ = std::move(other.state_);
        }
        return *this;
    }
    ~CollectionProvider() noexcept { close(); }

    static CollectionProvider create(Client& client, std::uint64_t node, const CollectionMetadata& metadata) {
        auto state = std::make_shared<State>();
        auto args = metadata.to_value();
        args.set("node", Value::string(std::to_string(node)));
        auto subscription = client.on(node, "data.request", [state](const Value& event) {
            try {
                auto request = CollectionRequest::from_event(event);
                std::lock_guard<std::mutex> lock(state->mutex);
                if (state->closed) return;
                if (state->requests.size() >= 1) {
                    state->failure = std::make_exception_ptr(BridgeError("queue_full", "Collection provider request queue is full", "", ""));
                } else {
                    state->requests.push_back(std::move(request));
                }
                state->ready.notify_all();
            } catch (...) {
                std::lock_guard<std::mutex> lock(state->mutex);
                if (!state->closed) { state->failure = std::current_exception(); state->ready.notify_all(); }
            }
        });
        (void)client.request("collection.create", args);
        return CollectionProvider(client, node, std::move(subscription), std::move(state));
    }

    std::optional<CollectionRequest> next_request(std::uint32_t timeout_ms = 30000) {
        if (!state_) return std::nullopt;
        std::unique_lock<std::mutex> lock(state_->mutex);
        if (state_->reader_waiting) throw BridgeError("invalid_lifecycle", "Only one collection reader is allowed", "", "");
        state_->reader_waiting = true;
        const auto ready = state_->ready.wait_for(lock, std::chrono::milliseconds(timeout_ms), [&] {
            return !state_->requests.empty() || state_->failure || state_->closed;
        });
        state_->reader_waiting = false;
        if (!ready) return std::nullopt;
        if (!state_->requests.empty()) {
            auto request = std::move(state_->requests.front()); state_->requests.pop_front(); return request;
        }
        if (state_->failure) { auto failure = std::exchange(state_->failure, {}); lock.unlock(); std::rethrow_exception(failure); }
        return std::nullopt;
    }

    void provide(const CollectionRequest& request, Value rows) {
        ensure_open();
        if (rows.size() > 512) throw BridgeError("quota_exceeded", "Too many collection rows", "rows", "");
        auto args = Value::object();
        args.set("node", Value::string(std::to_string(node_)));
        args.set("revision", Value::string(request.revision));
        args.set("request", Value::string(request.request));
        args.set("start", Value::i64(request.start));
        args.set("rows", std::move(rows));
        (void)client_->request("data.provide", args);
    }

    void update(const CollectionMetadata& metadata) {
        ensure_open();
        auto args = Value::object();
        args.set("node", Value::string(std::to_string(node_)));
        args.set("metadata", metadata.to_value());
        (void)client_->request("collection.update", args);
    }

    void close() noexcept {
        if (!state_) return;
        {
            std::lock_guard<std::mutex> lock(state_->mutex);
            if (!state_->closed) { state_->closed = true; state_->requests.clear(); state_->failure = nullptr; }
        }
        state_->ready.notify_all();
        subscription_.close();
        state_.reset(); client_ = nullptr;
    }

private:
    CollectionProvider(Client& client, std::uint64_t node, Subscription subscription, std::shared_ptr<State> state)
        : client_(&client), node_(node), subscription_(std::move(subscription)), state_(std::move(state)) {}
    Client* client_{};
    std::uint64_t node_{};
    Subscription subscription_;
    std::shared_ptr<State> state_;
    void ensure_open() const {
        if (!state_ || !client_) throw BridgeError("connection_closed", "Collection provider is closed", "", "");
        std::lock_guard<std::mutex> lock(state_->mutex);
        if (state_->closed) throw BridgeError("connection_closed", "Collection provider is closed", "", "");
    }
};

class Application {
    struct RunContext {
        std::function<void(Client&)> setup;
        std::exception_ptr exception;
        Client client;
        RunContext(gf_client* client_handle, std::function<void(Client&)> callback)
            : setup(std::move(callback)), client(client_handle) {}
    };
public:
    Application() { check(gf_client_create(&client_)); }
    Application(const Application&) = delete;
    Application& operator=(const Application&) = delete;
    Application(Application&& other) noexcept : client_(std::exchange(other.client_, nullptr)) {}
    Application& operator=(Application&& other) noexcept { if (this != &other) { reset(); client_ = std::exchange(other.client_, nullptr); } return *this; }
    ~Application() noexcept { reset(); }
    void submit(std::string_view jsonLine) { check(gf_client_submit(client_, reinterpret_cast<const uint8_t*>(jsonLine.data()), jsonLine.size())); }
    void run() { check(gf_client_run(client_, nullptr, nullptr)); }
    void run(std::function<void(Client&)> setup) {
        RunContext context(client_, std::move(setup));
        check(gf_client_run(client_, setup_thunk, &context));
        if (context.exception) std::rethrow_exception(context.exception);
    }
    void stop() { check(gf_client_close(client_)); }
    std::vector<uint8_t> next(uint32_t timeoutMs = 0) {
        size_t required = 0; auto status = gf_client_next(client_, nullptr, 0, &required, timeoutMs);
        if (status == GRAFIKR_TIMEOUT || status == GRAFIKR_STOPPED) return {};
        if (status != GRAFIKR_BUFFER_TOO_SMALL) check(status);
        std::vector<uint8_t> buffer(required);
        check(gf_client_next(client_, buffer.data(), buffer.size(), &required, 0));
        return buffer;
    }
private:
    gf_client* client_{};
    static void GRAFIKR_CALL setup_thunk(gf_client* client, void* userdata) noexcept {
        auto* context = static_cast<RunContext*>(userdata);
        try {
            (void)client;
            context->setup(context->client);
        } catch (...) {
            context->exception = std::current_exception();
            (void)gf_client_stop(client);
        }
    }
    static void check(gf_status status) { if (status != GRAFIKR_OK) throw Error("Grafikr ABI call failed: " + std::to_string(status)); }
    void reset() noexcept { if (client_) { (void)gf_client_close(client_); (void)gf_client_destroy(client_); client_ = nullptr; } }
};
}
#endif

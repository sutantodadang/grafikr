#include "grafikr.hpp"
#include <cstdint>
#include <exception>
#include <limits>
#include <stdexcept>
#include <type_traits>

static_assert(!std::is_copy_constructible_v<grafikr::Application>);
static_assert(!std::is_copy_constructible_v<grafikr::Value>);
static_assert(!std::is_copy_constructible_v<grafikr::Subscription>);
static_assert(std::is_move_constructible_v<grafikr::Subscription>);
static_assert(!std::is_copy_constructible_v<grafikr::CollectionProvider>);
static_assert(std::is_move_constructible_v<grafikr::CollectionProvider>);

int main() {
    grafikr::detail::CallbackErrors callback_errors;
    auto callback_event = grafikr::Value::object();
    callback_event.set("source", grafikr::Value::string("17"));
    int callback_error_count = 0;
    callback_errors.set([&](std::exception_ptr error, const grafikr::Value& event) {
        try { std::rethrow_exception(error); }
        catch (const std::runtime_error& failure) {
            if (std::string(failure.what()) == "handler failed" && event.get("source").as_string() == "17")
                callback_error_count++;
        }
    });
    try { throw std::runtime_error("handler failed"); }
    catch (...) { callback_errors.report(std::current_exception(), callback_event); }
    if (callback_error_count != 1) return 14;
    callback_errors.set([](std::exception_ptr, const grafikr::Value&) { throw std::runtime_error("error hook failed"); });
    callback_errors.report(std::make_exception_ptr(std::runtime_error("handler failed")), callback_event);

    auto object = grafikr::Value::object();
    object.set("minimum", grafikr::Value::i64(std::numeric_limits<int64_t>::min()));
    if (object.get("minimum").as_i64() != std::numeric_limits<int64_t>::min()) return 1;
    if (object.to_json() != "{\"minimum\":-9223372036854775808}") return 2;
    auto values = grafikr::Value::array();
    values.push(grafikr::Value::boolean(true));
    values.push(grafikr::Value::number(2.5));
    values.push(grafikr::Value::string("雪"));
    if (values.size() != 3) return 3;
    if (!values.at(0).as_bool()) return 4;
    if (values.at(1).as_f64() != 2.5) return 5;
    if (values.at(2).as_string() != "雪") return 6;
    bool rejected_non_finite = false;
    try { (void)grafikr::Value::number(std::numeric_limits<double>::infinity()); }
    catch (const grafikr::Error&) { rejected_non_finite = true; }
    if (!rejected_non_finite) return 13;

    auto event = grafikr::Value::object();
    auto payload = grafikr::Value::object();
    payload.set("revision", grafikr::Value::string("3"));
    payload.set("request", grafikr::Value::string("9"));
    payload.set("start", grafikr::Value::i64(4));
    payload.set("count", grafikr::Value::i64(2));
    event.set("payload", std::move(payload));
    auto range = grafikr::CollectionRequest::from_event(event);
    if (range.revision != "3" || range.request != "9" || range.start != 4 || range.count != 2) return 9;
    auto metadata = grafikr::CollectionMetadata{2, 24.0, {"label"}}.to_value();
    if (metadata.get("row_count").as_i64() != 2 || metadata.get("columns").at(0).as_string() != "label") return 10;
    auto invalid_metadata = false;
    try { (void)grafikr::CollectionMetadata{1, 24.0, {}}.to_value(); }
    catch (const grafikr::BridgeError& error) { invalid_metadata = error.code() == "invalid_value"; }
    if (!invalid_metadata) return 11;
    auto invalid_event = grafikr::Value::object();
    auto invalid_payload = grafikr::Value::object();
    invalid_payload.set("revision", grafikr::Value::string("1"));
    invalid_payload.set("request", grafikr::Value::string("2"));
    invalid_payload.set("start", grafikr::Value::i64(0));
    invalid_payload.set("count", grafikr::Value::i64(513));
    invalid_event.set("payload", std::move(invalid_payload));
    auto invalid_range = false;
    try { (void)grafikr::CollectionRequest::from_event(invalid_event); }
    catch (const grafikr::BridgeError& error) { invalid_range = error.code() == "invalid_frame"; }
    if (!invalid_range) return 12;

    grafikr::Application app;
    app.stop();
    app.run();

    bool received_structured_error = false;
    bool failed_subscription_rejected = false;
    grafikr::Application high_level;
    high_level.run([&](grafikr::Client& client) {
        auto args = grafikr::Value::object();
        args.set("window", grafikr::Value::string("9999"));
        try {
            (void)client.request("window.close", args);
        } catch (const grafikr::BridgeError& error) {
            received_structured_error = error.code() == "invalid_handle";
        }
        try {
            (void)client.on(9999, "click", [](const grafikr::Value&) {});
        } catch (const grafikr::Error&) {
            failed_subscription_rejected = true;
        }
        (void)client.request("shutdown", grafikr::Value::object());
    });
    if (!received_structured_error) return 7;
    if (!failed_subscription_rejected) return 8;
    return 0;
}

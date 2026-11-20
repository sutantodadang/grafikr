#include "grafikr.hpp"

#include <atomic>
#include <cstdio>
#include <exception>
#include <exception>
#include <memory>
#include <optional>
#include <stdexcept>
#include <string>
#include <thread>

int main() {
    grafikr::Application app;
    std::unique_ptr<grafikr::CollectionProvider> provider;
    std::thread reader;
    std::optional<grafikr::Subscription> nested_subscription;
    std::optional<grafikr::Subscription> throwing_subscription;
    std::atomic<int> data_request_callbacks{};
    std::atomic<int> nested_request_succeeded{};
    std::atomic<int> callback_error_received{};
    bool supplied_range = false;

    app.run([&](grafikr::Client& client) {
        client.on_error([&](std::exception_ptr error, const grafikr::Value& event) {
            try {
                if (event.get("event").as_string() != "data.request") return;
                std::rethrow_exception(error);
            } catch (const std::runtime_error& failure) {
                if (std::string(failure.what()) == "callback error test") {
                    callback_error_received++;
                }
            }
        });
        auto window_args = grafikr::Value::object();
        window_args.set("title", grafikr::Value::string("C++ collection provider"));
        window_args.set("width", grafikr::Value::i64(400));
        window_args.set("height", grafikr::Value::i64(300));
        auto window = client.request("window.create", window_args);
        auto window_id = window.get("window").as_string();
        auto root = window.get("root").as_string();

        auto node_args = grafikr::Value::object();
        node_args.set("window", grafikr::Value::string(window_id));
        node_args.set("widget", grafikr::Value::string("VirtualList"));
        auto properties = grafikr::Value::object();
        properties.set("row_count", grafikr::Value::i64(50000));
        properties.set("row_height", grafikr::Value::number(24));
        properties.set("height", grafikr::Value::number(120));
        node_args.set("properties", std::move(properties));
        auto node = client.request("node.create", node_args);
        auto node_id = std::stoull(node.get("node").as_string());

        auto attach_args = grafikr::Value::object();
        attach_args.set("node", node.get("node"));
        attach_args.set("parent", grafikr::Value::string(root));
        (void)client.request("node.attach", attach_args);

        nested_subscription.emplace(client.on(node_id, "data.request", [&client, &data_request_callbacks, &nested_request_succeeded, &nested_subscription](const grafikr::Value&) {
            data_request_callbacks++;
            auto args = grafikr::Value::object();
            args.set("state", grafikr::Value::string("0"));
            try { (void)client.request("state.get", args); }
            catch (const grafikr::BridgeError& error) {
                if (error.code() == "invalid_handle") {
                    nested_request_succeeded++;
                    nested_subscription->close();
                }
            }
        }));
        throwing_subscription.emplace(client.on(node_id, "data.request", [](const grafikr::Value&) {
            throw std::runtime_error("callback error test");
        }));

        provider = std::make_unique<grafikr::CollectionProvider>(
            grafikr::CollectionProvider::create(client, node_id,
                grafikr::CollectionMetadata{50000, 24, {"label"}}));
        auto refresh_args = grafikr::Value::object();
        refresh_args.set("window", grafikr::Value::string(window_id));
        refresh_args.set("width", grafikr::Value::number(400));
        refresh_args.set("height", grafikr::Value::number(300));
        (void)client.request("window.update", refresh_args);
        reader = std::thread([&client, &provider, &supplied_range, &nested_subscription, &throwing_subscription] {
            try {
                for (int request_index = 0; request_index < 1; ++request_index) {
                    auto range = provider->next_request(30000);
                    if (!range || range->count == 0 || range->count > 512) throw grafikr::Error("missing visible range");
                    auto rows = grafikr::Value::array();
                    for (std::uint32_t i = 0; i < range->count; ++i) {
                        auto row = grafikr::Value::array();
                        row.push(grafikr::Value::string("row-" + std::to_string(range->start + i)));
                        rows.push(std::move(row));
                    }
                    provider->provide(*range, std::move(rows));
                    supplied_range = true;
                }
                provider->close();
            } catch (const std::exception& error) {
                std::fprintf(stderr, "C++ provider reader failed: %s\n", error.what());
                provider->close();
            } catch (...) {
                std::fprintf(stderr, "C++ provider reader failed with unknown exception\n");
                provider->close();
            }
            nested_subscription->close();
            throwing_subscription->close();
            (void)client.request("shutdown", grafikr::Value::object());
        });
    });

    if (reader.joinable()) reader.join();
    provider.reset();
    if (!supplied_range || data_request_callbacks != 1 || nested_request_succeeded != 1 || callback_error_received != 1) {
        std::fprintf(stderr, "C++ provider scenario failed (range=%d, callbacks=%d, nested=%d, callback_error=%d)\n",
                     supplied_range, data_request_callbacks.load(), nested_request_succeeded.load(), callback_error_received.load());
        return 1;
    }
    return 0;
}

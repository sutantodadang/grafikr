#include "grafikr.hpp"
#include "mock_ffi.h"

#include <string>
#include <thread>

int main() {
    mock_ffi_set_mode(MOCK_FFI_OUT_OF_ORDER);
    grafikr::Application app;
    std::string first;
    std::string second;
    app.run([&](grafikr::Client& client) {
        auto request = [&](const char* value, std::string& output) {
            auto args = grafikr::Value::object();
            args.set("state", grafikr::Value::string(value));
            output = client.request("state.get", args).get("value").as_string();
        };
        std::thread a(request, "first", std::ref(first));
        std::thread b(request, "second", std::ref(second));
        a.join();
        b.join();
        app.stop();
    });
    if (first != "first" || second != "second") return 1;

    return 0;
}

#pragma once

#include <atomic>
#include <cstdint>
#include <mutex>
#include <string>
#include <thread>
#include <vector>

class BridgeServer {
public:
    BridgeServer() = default;
    ~BridgeServer();

    void start(int port);
    void stop();
    void publish(std::string json);

#ifdef _WIN32
    using socket_t = std::uintptr_t;
#else
    using socket_t = int;
#endif
    static constexpr socket_t kInvalid = (socket_t)-1;

private:
    void run();
    void closeSocket(socket_t s);

    std::atomic<bool> running_{false};
    std::thread thread_;
    int port_ = 0;

    std::mutex snap_mu_;
    std::string latest_;
    std::uint64_t latest_seq_ = 0;

    std::vector<socket_t> clients_;
    std::vector<std::uint64_t> client_seq_;
};

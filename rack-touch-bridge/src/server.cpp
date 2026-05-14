#include "server.hpp"

#include <algorithm>
#include <chrono>
#include <cstring>
#include <utility>

#ifdef _WIN32
#  include <winsock2.h>
#  include <ws2tcpip.h>
   using socklen_t_ = int;
#  define BS_CLOSE closesocket
#  define BS_ERR_WOULDBLOCK WSAEWOULDBLOCK
#  define BS_LAST_ERR WSAGetLastError()
#else
#  include <arpa/inet.h>
#  include <errno.h>
#  include <fcntl.h>
#  include <netinet/in.h>
#  include <sys/select.h>
#  include <sys/socket.h>
#  include <sys/types.h>
#  include <unistd.h>
   using socklen_t_ = socklen_t;
#  define BS_CLOSE ::close
#  define BS_ERR_WOULDBLOCK EWOULDBLOCK
#  define BS_LAST_ERR errno
#endif

namespace {

bool setNonBlocking(BridgeServer::socket_t s) {
#ifdef _WIN32
    u_long mode = 1;
    return ioctlsocket((SOCKET)s, FIONBIO, &mode) == 0;
#else
    int flags = fcntl(s, F_GETFL, 0);
    if (flags == -1) return false;
    return fcntl(s, F_SETFL, flags | O_NONBLOCK) == 0;
#endif
}

}  // namespace

BridgeServer::~BridgeServer() {
    stop();
}

void BridgeServer::start(int port) {
    if (running_.exchange(true)) return;
    port_ = port;
    thread_ = std::thread(&BridgeServer::run, this);
}

void BridgeServer::stop() {
    if (!running_.exchange(false)) return;
    if (thread_.joinable()) thread_.join();
}

void BridgeServer::publish(std::string json) {
    std::lock_guard<std::mutex> lk(snap_mu_);
    latest_ = std::move(json);
    latest_seq_++;
}

void BridgeServer::closeSocket(socket_t s) {
    if (s == kInvalid) return;
    BS_CLOSE((decltype(BS_CLOSE(0)))s);
}

void BridgeServer::run() {
#ifdef _WIN32
    WSADATA wsa;
    if (WSAStartup(MAKEWORD(2, 2), &wsa) != 0) {
        running_ = false;
        return;
    }
#endif

    socket_t listen_sock = (socket_t)::socket(AF_INET, SOCK_STREAM, 0);
    if (listen_sock == kInvalid) {
#ifdef _WIN32
        WSACleanup();
#endif
        running_ = false;
        return;
    }

    int yes = 1;
    setsockopt((int)listen_sock, SOL_SOCKET, SO_REUSEADDR,
               (const char*)&yes, sizeof(yes));
    setNonBlocking(listen_sock);

    sockaddr_in addr{};
    addr.sin_family = AF_INET;
    addr.sin_port = htons((std::uint16_t)port_);
    addr.sin_addr.s_addr = htonl(INADDR_LOOPBACK);

    if (::bind((int)listen_sock, (sockaddr*)&addr, sizeof(addr)) != 0 ||
        ::listen((int)listen_sock, 4) != 0) {
        closeSocket(listen_sock);
#ifdef _WIN32
        WSACleanup();
#endif
        running_ = false;
        return;
    }

    while (running_.load()) {
        fd_set rfds;
        FD_ZERO(&rfds);
        FD_SET((int)listen_sock, &rfds);
        int maxfd = (int)listen_sock;

        timeval tv{};
        tv.tv_sec = 0;
        tv.tv_usec = 50 * 1000;
        ::select(maxfd + 1, &rfds, nullptr, nullptr, &tv);

        if (FD_ISSET((int)listen_sock, &rfds)) {
            sockaddr_in caddr{};
            socklen_t_ clen = sizeof(caddr);
            socket_t cs = (socket_t)::accept((int)listen_sock,
                                             (sockaddr*)&caddr, &clen);
            if (cs != kInvalid) {
                setNonBlocking(cs);
                clients_.push_back(cs);
                client_seq_.push_back(0);
            }
        }

        std::string snap;
        std::uint64_t seq = 0;
        {
            std::lock_guard<std::mutex> lk(snap_mu_);
            snap = latest_;
            seq = latest_seq_;
        }
        if (snap.empty()) continue;
        if (snap.back() != '\n') snap.push_back('\n');

        for (std::size_t i = 0; i < clients_.size();) {
            if (client_seq_[i] == seq) { ++i; continue; }
#ifdef _WIN32
            int sent = ::send((SOCKET)clients_[i], snap.data(),
                              (int)snap.size(), 0);
#else
            ssize_t sent = ::send((int)clients_[i], snap.data(),
                                  snap.size(), MSG_NOSIGNAL);
#endif
            if (sent < 0 && BS_LAST_ERR == BS_ERR_WOULDBLOCK) {
                ++i;
                continue;
            }
            if (sent < 0) {
                closeSocket(clients_[i]);
                clients_.erase(clients_.begin() + i);
                client_seq_.erase(client_seq_.begin() + i);
                continue;
            }
            client_seq_[i] = seq;
            ++i;
        }
    }

    for (socket_t c : clients_) closeSocket(c);
    clients_.clear();
    client_seq_.clear();
    closeSocket(listen_sock);
#ifdef _WIN32
    WSACleanup();
#endif
}

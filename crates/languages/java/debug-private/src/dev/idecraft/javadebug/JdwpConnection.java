// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0
package dev.idecraft.javadebug;

import java.io.DataInputStream;
import java.io.DataOutputStream;
import java.io.EOFException;
import java.io.IOException;
import java.net.Socket;
import java.net.SocketException;
import java.net.SocketTimeoutException;
import java.nio.charset.StandardCharsets;
import java.util.Arrays;

import com.sun.jdi.connect.spi.ClosedConnectionException;
import com.sun.jdi.connect.spi.Connection;

/** JDWP packets (four length bytes, big endian, counting themselves) over a connected socket. */
final class JdwpConnection extends Connection {
    private static final byte[] HANDSHAKE = "JDWP-Handshake".getBytes(StandardCharsets.US_ASCII);
    private static final int HANDSHAKE_MILLIS = 5_000;

    private final Socket socket;
    private final DataInputStream input;
    private final DataOutputStream output;

    JdwpConnection(Socket socket) throws IOException {
        this.socket = socket;
        this.input = new DataInputStream(socket.getInputStream());
        this.output = new DataOutputStream(socket.getOutputStream());
    }

    /** Whether the peer speaks JDWP; the caller drops anything else that reached the port. */
    static boolean handshake(Socket peer) throws IOException {
        peer.setSoTimeout(HANDSHAKE_MILLIS);
        // The debugger speaks first: the program's agent waits for these bytes before it answers.
        peer.getOutputStream().write(HANDSHAKE);
        byte[] received = new byte[HANDSHAKE.length];
        try {
            new DataInputStream(peer.getInputStream()).readFully(received);
        } catch (EOFException | SocketTimeoutException stranger) {
            return false;
        }
        if (!Arrays.equals(received, HANDSHAKE)) {
            return false;
        }
        peer.setSoTimeout(0);
        return true;
    }

    @Override
    public void close() throws IOException {
        socket.close();
    }

    @Override
    public boolean isOpen() {
        return !socket.isClosed();
    }

    @Override
    public byte[] readPacket() throws IOException {
        try {
            int length = input.readInt();
            if (length < 4) {
                throw new IOException("Bad JDWP packet length " + length);
            }
            byte[] packet = new byte[length];
            packet[0] = (byte) (length >>> 24);
            packet[1] = (byte) (length >>> 16);
            packet[2] = (byte) (length >>> 8);
            packet[3] = (byte) length;
            input.readFully(packet, 4, length - 4);
            return packet;
        } catch (EOFException | SocketException closed) {
            throw new ClosedConnectionException();
        }
    }

    @Override
    public void writePacket(byte[] packet) throws IOException {
        synchronized (output) {
            output.write(packet);
            output.flush();
        }
    }
}

// SPDX-License-Identifier: Apache-2.0
package dev.ferrisidea.javadebug;

import java.util.List;

import com.microsoft.java.debug.plugin.internal.AdvancedVirtualMachineManager;
import com.sun.jdi.connect.LaunchingConnector;
import com.sun.jdi.connect.ListeningConnector;

/**
 * Launches only through {@link LoopbackLaunchingConnector} and offers no listening connector, as
 * the platform's bind every interface. Ferris IDEA's launches never ask for one (the Debug console
 * stays inside the IDE); a request for one fails instead of opening a port.
 */
final class LoopbackVirtualMachineManager extends AdvancedVirtualMachineManager {
    @Override
    public List<LaunchingConnector> launchingConnectors() {
        return List.of(new LoopbackLaunchingConnector(this));
    }

    @Override
    public List<ListeningConnector> listeningConnectors() {
        return List.of();
    }
}

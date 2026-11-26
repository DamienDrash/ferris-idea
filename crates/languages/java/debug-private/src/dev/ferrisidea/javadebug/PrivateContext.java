// SPDX-License-Identifier: Apache-2.0
package dev.ferrisidea.javadebug;

import com.microsoft.java.debug.core.adapter.IProvider;
import com.microsoft.java.debug.core.adapter.IProviderContext;
import com.microsoft.java.debug.core.adapter.IVirtualMachineManagerProvider;
import com.microsoft.java.debug.plugin.internal.JdtProviderContextFactory;
import com.sun.jdi.VirtualMachineManager;

/** The plugin's providers, with the virtual machine manager exchanged for the loopback-only one. */
final class PrivateContext implements IProviderContext {
    private static final IVirtualMachineManagerProvider MANAGERS = new IVirtualMachineManagerProvider() {
        private final VirtualMachineManager manager = new LoopbackVirtualMachineManager();

        @Override
        public VirtualMachineManager getVirtualMachineManager() {
            return manager;
        }
    };

    private final IProviderContext plugin = JdtProviderContextFactory.createProviderContext();

    @Override
    public <T extends IProvider> T getProvider(Class<T> type) {
        return type == IVirtualMachineManagerProvider.class ? type.cast(MANAGERS) : plugin.getProvider(type);
    }

    @Override
    public void registerProvider(Class<? extends IProvider> type, IProvider provider) {
        plugin.registerProvider(type, provider);
    }
}

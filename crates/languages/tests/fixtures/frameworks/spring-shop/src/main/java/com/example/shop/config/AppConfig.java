package com.example.shop.config;

import com.example.shop.payment.PaymentGateway;
import com.example.shop.tax.FlatTax;
import com.example.shop.tax.NoTax;
import com.example.shop.tax.TaxCalculator;
import java.time.Clock;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import org.springframework.beans.factory.annotation.Qualifier;
import org.springframework.context.annotation.Bean;
import org.springframework.context.annotation.Configuration;
import org.springframework.context.annotation.Profile;

@Configuration
public class AppConfig {

    @Bean
    public Clock clock() {
        return Clock.systemUTC();
    }

    @Bean(destroyMethod = "shutdown")
    public ExecutorService workers() {
        return Executors.newFixedThreadPool(2);
    }

    @Bean(name = {"receipts", "receiptPrinter"})
    public ReceiptPrinter receiptPrinter(@Qualifier("stripeGateway") PaymentGateway gateway, Clock clock) {
        return new ReceiptPrinter();
    }

    // Two beans of one type, one per profile: never both active.
    @Bean
    @Profile("prod")
    public TaxCalculator flatTax() {
        return new FlatTax();
    }

    @Bean
    @Profile("!prod")
    public TaxCalculator noTax() {
        return new NoTax();
    }

    public static class ReceiptPrinter {
    }
}

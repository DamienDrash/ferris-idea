package com.example.shop.repository;

import com.example.shop.model.Order;
import org.springframework.data.jpa.repository.JpaRepository;

// A Spring Data repository: an interface, the framework provides the bean.
public interface OrderRepository extends JpaRepository<Order, Long> {
}

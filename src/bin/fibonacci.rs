/// 递归实现（适合小 n）
pub fn fib_recursive(n: u64) -> u64 {
    match n {
        0 => 0,
        1 => 1,
        _ => fib_recursive(n - 1) + fib_recursive(n - 2),
    }
}

/// 迭代实现（高效，O(n) 时间，O(1) 空间）
pub fn fib_iter(n: u64) -> u64 {
    let (mut a, mut b) = (0u64, 1u64);
    for _ in 0..n {
        (a, b) = (b, a + b);
    }
    a
}

fn main() {
    println!("{:<6} {:<20} {}", "n", "迭代", "递归（仅 n≤30）");
    println!("{}", "-".repeat(40));
    for n in [0, 1, 2, 5, 10, 20, 30] {
        println!("{:<6} {:<20} {}", n, fib_iter(n), fib_recursive(n));
    }

    // 大数只用迭代版
    println!("\nfib(50)  = {}", fib_iter(50));
    println!("fib(93)  = {}", fib_iter(93)); // u64 最大可表示值附近
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base_cases() {
        assert_eq!(fib_iter(0), 0);
        assert_eq!(fib_iter(1), 1);
    }

    #[test]
    fn test_known_values() {
        let expected = [0, 1, 1, 2, 3, 5, 8, 13, 21, 34];
        for (n, &val) in expected.iter().enumerate() {
            assert_eq!(fib_iter(n as u64), val);
            assert_eq!(fib_recursive(n as u64), val);
        }
    }

    #[test]
    fn test_large_iter() {
        assert_eq!(fib_iter(50), 12_586_269_025);
    }
}

#![no_std]
#![no_main]

// quick sort
#[unsafe(no_mangle)]
fn main() {
    let mut array = [7, 4, 2, 9, 1, 0, 8, 6, 5, 3];
    athera_userland::println!("array: {array:?}");
    quick_sort(&mut array);
    athera_userland::println!("sorted array: {array:?}")
}

/// 原地快速排序（Lomuto 分区方案）
fn quick_sort(arr: &mut [i32]) {
    if arr.len() <= 1 {
        return;
    }
    // 选择最后一个元素作为基准
    let pivot = arr[arr.len() - 1];
    let mut i = 0;
    // 分区：将小于等于 pivot 的元素放到左侧
    for j in 0..arr.len() - 1 {
        if arr[j] <= pivot {
            arr.swap(i, j);
            i += 1;
        }
    }
    // 将基准放到正确位置
    arr.swap(i, arr.len() - 1);
    // 递归排序左右两部分
    quick_sort(&mut arr[..i]);
    quick_sort(&mut arr[i + 1..]);
}

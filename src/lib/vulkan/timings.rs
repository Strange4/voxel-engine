use std::sync::Arc;

use vulkano::query::{QueryPool, QueryResultFlags};

pub fn get_query_timings(
    query_pool: &Arc<QueryPool>,
    query_index_start: u32,
    number_of_queries: u32,
    timestamp_period: f64,
    wait_for_query: bool,
) -> Option<Vec<f64>> {
    if wait_for_query {
        let mut timing_results: Vec<u64> = vec![0; number_of_queries as usize];
        query_pool
            .get_results(
                query_index_start..query_index_start + number_of_queries,
                &mut timing_results,
                QueryResultFlags::WAIT,
            )
            .unwrap();
        let results = timing_results
            .into_iter()
            .map(|value| value as f64 * timestamp_period)
            .collect();
        return Some(results);
    }

    let mut timing_results: Vec<u64> = vec![0; (number_of_queries * 2) as usize];
    query_pool
        .get_results(
            query_index_start..query_index_start + number_of_queries,
            &mut timing_results,
            QueryResultFlags::WITH_AVAILABILITY,
        )
        .unwrap();
    let all_available = !timing_results
        .iter()
        .enumerate()
        .any(|(index, &value)| index % 2 == 1 && value == 0);

    if !all_available {
        return None;
    }
    let results = timing_results
        .into_iter()
        .enumerate()
        .filter_map(|(index, value)| {
            if index % 2 == 1 {
                None
            } else {
                Some(value as f64 * timestamp_period)
            }
        })
        .collect();
    Some(results)
}

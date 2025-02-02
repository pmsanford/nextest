// Copyright (c) The nextest Contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use crate::fixtures::*;
use color_eyre::eyre::Result;
use nextest_runner::{
    config::NextestConfig,
    runner::{ExecutionDescription, ExecutionResult, TestRunnerBuilder},
    signal::SignalHandler,
    test_filter::{RunIgnored, TestFilterBuilder},
    test_list::TestList,
};
use pretty_assertions::assert_eq;

#[test]
fn test_list_tests() -> Result<()> {
    let test_filter = TestFilterBuilder::any(RunIgnored::Default);
    let test_bins: Vec<_> = FIXTURE_TARGETS.values().cloned().collect();
    let test_list = TestList::new(test_bins, &test_filter, None)?;

    for (name, expected) in &*EXPECTED_TESTS {
        let test_binary = FIXTURE_TARGETS
            .get(*name)
            .unwrap_or_else(|| panic!("unexpected test name {}", name));
        let info = test_list
            .get(&test_binary.binary_path)
            .unwrap_or_else(|| panic!("test list not found for {}", test_binary.binary_path));
        let tests: Vec<_> = info
            .testcases
            .iter()
            .map(|(name, info)| (name.as_str(), info.filter_match))
            .collect();
        assert_eq!(expected, &tests, "test list matches");
    }

    Ok(())
}

#[test]
fn test_run() -> Result<()> {
    let test_filter = TestFilterBuilder::any(RunIgnored::Default);
    let test_bins: Vec<_> = FIXTURE_TARGETS.values().cloned().collect();
    let test_list = TestList::new(test_bins, &test_filter, None)?;
    let config =
        NextestConfig::from_sources(&workspace_root(), None).expect("loaded fixture config");
    let profile = config
        .profile(NextestConfig::DEFAULT_PROFILE)
        .expect("default config is valid");

    let runner = TestRunnerBuilder::default().build(&test_list, &profile, SignalHandler::noop());

    let (instance_statuses, run_stats) = execute_collect(&runner);

    for (name, expected) in &*EXPECTED_TESTS {
        let test_binary = FIXTURE_TARGETS
            .get(*name)
            .unwrap_or_else(|| panic!("unexpected test name {}", name));
        for fixture in expected {
            let instance_value =
                &instance_statuses[&(test_binary.binary_path.as_path(), fixture.name)];
            let valid = match &instance_value.status {
                InstanceStatus::Skipped(_) => fixture.status.is_ignored(),
                InstanceStatus::Finished(run_statuses) => {
                    // This test should not have been retried since retries aren't configured.
                    assert_eq!(
                        run_statuses.len(),
                        1,
                        "test {} should have been run exactly once",
                        fixture.name
                    );
                    let run_status = run_statuses.last_status();
                    run_status.result == fixture.status.to_test_status(1)
                }
            };
            if !valid {
                panic!(
                    "for test {}, mismatch in status: expected {:?}, actual {:?}",
                    fixture.name, fixture.status, instance_value.status
                );
            }
        }
    }

    assert!(!run_stats.is_success(), "run should be marked failed");
    Ok(())
}

#[test]
fn test_run_ignored() -> Result<()> {
    let test_filter = TestFilterBuilder::any(RunIgnored::IgnoredOnly);
    let test_bins: Vec<_> = FIXTURE_TARGETS.values().cloned().collect();
    let test_list = TestList::new(test_bins, &test_filter, None)?;
    let config =
        NextestConfig::from_sources(&workspace_root(), None).expect("loaded fixture config");
    let profile = config
        .profile(NextestConfig::DEFAULT_PROFILE)
        .expect("default config is valid");

    let runner = TestRunnerBuilder::default().build(&test_list, &profile, SignalHandler::noop());

    let (instance_statuses, run_stats) = execute_collect(&runner);

    for (name, expected) in &*EXPECTED_TESTS {
        let test_binary = FIXTURE_TARGETS
            .get(*name)
            .unwrap_or_else(|| panic!("unexpected test name {}", name));
        for fixture in expected {
            let instance_value =
                &instance_statuses[&(test_binary.binary_path.as_path(), fixture.name)];
            let valid = match &instance_value.status {
                InstanceStatus::Skipped(_) => !fixture.status.is_ignored(),
                InstanceStatus::Finished(run_statuses) => {
                    // This test should not have been retried since retries aren't configured.
                    assert_eq!(
                        run_statuses.len(),
                        1,
                        "test {} should have been run exactly once",
                        fixture.name
                    );
                    let run_status = run_statuses.last_status();
                    run_status.result == fixture.status.to_test_status(1)
                }
            };
            if !valid {
                panic!(
                    "for test {}, mismatch in status: expected {:?}, actual {:?}",
                    fixture.name, fixture.status, instance_value.status
                );
            }
        }
    }

    assert!(!run_stats.is_success(), "run should be marked failed");
    Ok(())
}

#[test]
fn test_retries() -> Result<()> {
    let test_filter = TestFilterBuilder::any(RunIgnored::Default);
    let test_bins: Vec<_> = FIXTURE_TARGETS.values().cloned().collect();
    let test_list = TestList::new(test_bins, &test_filter, None)?;
    let config =
        NextestConfig::from_sources(&workspace_root(), None).expect("loaded fixture config");
    let profile = config
        .profile("with-retries")
        .expect("with-retries config is valid");

    let retries = profile.retries();
    assert_eq!(retries, 2, "retries set in with-retries profile");

    let runner = TestRunnerBuilder::default().build(&test_list, &profile, SignalHandler::noop());

    let (instance_statuses, run_stats) = execute_collect(&runner);

    for (name, expected) in &*EXPECTED_TESTS {
        let test_binary = FIXTURE_TARGETS
            .get(*name)
            .unwrap_or_else(|| panic!("unexpected test name {}", name));
        for fixture in expected {
            let instance_value =
                &instance_statuses[&(test_binary.binary_path.as_path(), fixture.name)];
            let valid = match &instance_value.status {
                InstanceStatus::Skipped(_) => fixture.status.is_ignored(),
                InstanceStatus::Finished(run_statuses) => {
                    let expected_len = match fixture.status {
                        FixtureStatus::Flaky { pass_attempt } => pass_attempt,
                        FixtureStatus::Pass => 1,
                        FixtureStatus::Fail => retries + 1,
                        FixtureStatus::IgnoredPass | FixtureStatus::IgnoredFail => {
                            unreachable!("ignored tests should be skipped")
                        }
                    };
                    assert_eq!(
                        run_statuses.len(),
                        expected_len,
                        "test {} should be run {} times",
                        fixture.name,
                        expected_len,
                    );

                    match run_statuses.describe() {
                        ExecutionDescription::Success { single_status } => {
                            single_status.result == ExecutionResult::Pass
                        }
                        ExecutionDescription::Flaky {
                            last_status,
                            prior_statuses,
                        } => {
                            assert_eq!(
                                prior_statuses.len(),
                                expected_len - 1,
                                "correct length for prior statuses"
                            );
                            for prior_status in prior_statuses {
                                assert_eq!(
                                    prior_status.result,
                                    ExecutionResult::Fail,
                                    "prior status {} should be fail",
                                    prior_status.attempt
                                );
                            }
                            last_status.result == ExecutionResult::Pass
                        }
                        ExecutionDescription::Failure {
                            first_status,
                            retries,
                            ..
                        } => {
                            assert_eq!(
                                retries.len(),
                                expected_len - 1,
                                "correct length for retries"
                            );
                            for retry in retries {
                                assert_eq!(
                                    retry.result,
                                    ExecutionResult::Fail,
                                    "retry {} should be fail",
                                    retry.attempt
                                );
                            }
                            first_status.result == ExecutionResult::Fail
                        }
                    }
                }
            };
            if !valid {
                panic!(
                    "for test {}, mismatch in status: expected {:?}, actual {:?}",
                    fixture.name, fixture.status, instance_value.status
                );
            }
        }
    }

    assert!(!run_stats.is_success(), "run should be marked failed");
    Ok(())
}

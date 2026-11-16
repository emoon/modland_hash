/* SPDX-License-Identifier: BSL-1.0 OR BSD-3-Clause */

#ifndef MPT_BASE_TESTS_NUMERIC_HPP
#define MPT_BASE_TESTS_NUMERIC_HPP



#include "mpt/base/detect_compiler.hpp"
#include "mpt/base/integer.hpp"
#include "mpt/base/namespace.hpp"
#include "mpt/base/numeric.hpp"
#include "mpt/test/test.hpp"
#include "mpt/test/test_macros.hpp"

#include <limits>



namespace mpt {
inline namespace MPT_INLINE_NS {



namespace tests {
namespace base {
namespace numeric {

#if MPT_COMPILER_CLANG
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wglobal-constructors"
#endif
MPT_TEST_GROUP_INLINE("mpt/base/numeric")
#if MPT_COMPILER_CLANG
#pragma clang diagnostic pop
#endif
{

	MPT_TEST_EXPECT_EQUAL(mpt::saturate_align_up<uint32>(0u, 4u), 0u);
	MPT_TEST_EXPECT_EQUAL(mpt::saturate_align_up<uint32>(1u, 4u), 4u);
	MPT_TEST_EXPECT_EQUAL(mpt::saturate_align_up<uint32>(2u, 4u), 4u);
	MPT_TEST_EXPECT_EQUAL(mpt::saturate_align_up<uint32>(3u, 4u), 4u);
	MPT_TEST_EXPECT_EQUAL(mpt::saturate_align_up<uint32>(4u, 4u), 4u);
	MPT_TEST_EXPECT_EQUAL(mpt::saturate_align_up<uint32>(5u, 4u), 8u);

	MPT_TEST_EXPECT_EQUAL(mpt::saturate_align_up<uint32>(std::numeric_limits<uint32>::max() - 5u, 4u), std::numeric_limits<uint32>::max() - 3u);
	MPT_TEST_EXPECT_EQUAL(mpt::saturate_align_up<uint32>(std::numeric_limits<uint32>::max() - 4u, 4u), std::numeric_limits<uint32>::max() - 3u);
	MPT_TEST_EXPECT_EQUAL(mpt::saturate_align_up<uint32>(std::numeric_limits<uint32>::max() - 3u, 4u), std::numeric_limits<uint32>::max() - 3u);
	MPT_TEST_EXPECT_EQUAL(mpt::saturate_align_up<uint32>(std::numeric_limits<uint32>::max() - 2u, 4u), std::numeric_limits<uint32>::max());
	MPT_TEST_EXPECT_EQUAL(mpt::saturate_align_up<uint32>(std::numeric_limits<uint32>::max() - 1u, 4u), std::numeric_limits<uint32>::max());
	MPT_TEST_EXPECT_EQUAL(mpt::saturate_align_up<uint32>(std::numeric_limits<uint32>::max() - 0u, 4u), std::numeric_limits<uint32>::max());

	MPT_TEST_EXPECT_EQUAL(mpt::saturate_align_up<int32>(0, 4), 0);
	MPT_TEST_EXPECT_EQUAL(mpt::saturate_align_up<int32>(1, 4), 4);
	MPT_TEST_EXPECT_EQUAL(mpt::saturate_align_up<int32>(2, 4), 4);
	MPT_TEST_EXPECT_EQUAL(mpt::saturate_align_up<int32>(3, 4), 4);
	MPT_TEST_EXPECT_EQUAL(mpt::saturate_align_up<int32>(4, 4), 4);
	MPT_TEST_EXPECT_EQUAL(mpt::saturate_align_up<int32>(5, 4), 8);

	MPT_TEST_EXPECT_EQUAL(mpt::saturate_align_up<int32>(std::numeric_limits<int32>::max() - 5, 4), std::numeric_limits<int32>::max() - 3);
	MPT_TEST_EXPECT_EQUAL(mpt::saturate_align_up<int32>(std::numeric_limits<int32>::max() - 4, 4), std::numeric_limits<int32>::max() - 3);
	MPT_TEST_EXPECT_EQUAL(mpt::saturate_align_up<int32>(std::numeric_limits<int32>::max() - 3, 4), std::numeric_limits<int32>::max() - 3);
	MPT_TEST_EXPECT_EQUAL(mpt::saturate_align_up<int32>(std::numeric_limits<int32>::max() - 2, 4), std::numeric_limits<int32>::max());
	MPT_TEST_EXPECT_EQUAL(mpt::saturate_align_up<int32>(std::numeric_limits<int32>::max() - 1, 4), std::numeric_limits<int32>::max());
	MPT_TEST_EXPECT_EQUAL(mpt::saturate_align_up<int32>(std::numeric_limits<int32>::max() - 0, 4), std::numeric_limits<int32>::max());

	MPT_TEST_EXPECT_EQUAL(mpt::midpoint(2, 3), 2);
	MPT_TEST_EXPECT_EQUAL(mpt::midpoint(3, 2), 3);
	MPT_TEST_EXPECT_EQUAL(mpt::midpoint(2, 4), 3);
	MPT_TEST_EXPECT_EQUAL(mpt::midpoint(4, 2), 3);
	MPT_TEST_EXPECT_EQUAL(mpt::midpoint(-1, 1), 0);
	MPT_TEST_EXPECT_EQUAL(mpt::midpoint(1, -1), 0);
	MPT_TEST_EXPECT_EQUAL(mpt::midpoint(std::numeric_limits<int>::min(), std::numeric_limits<int>::max()), -1);
	MPT_TEST_EXPECT_EQUAL(mpt::midpoint(std::numeric_limits<int>::max(), std::numeric_limits<int>::min()), 0);
	MPT_TEST_EXPECT_EQUAL(mpt::midpoint(std::numeric_limits<int>::min() + 2, std::numeric_limits<int>::min()), std::numeric_limits<int>::min() + 1);
	MPT_TEST_EXPECT_EQUAL(mpt::midpoint(std::numeric_limits<int>::min(), std::numeric_limits<int>::min() + 2), std::numeric_limits<int>::min() + 1);
	MPT_TEST_EXPECT_EQUAL(mpt::midpoint(std::numeric_limits<int>::max() - 2, std::numeric_limits<int>::max()), std::numeric_limits<int>::max() - 1);
	MPT_TEST_EXPECT_EQUAL(mpt::midpoint(std::numeric_limits<int>::max(), std::numeric_limits<int>::max() - 2), std::numeric_limits<int>::max() - 1);
	unsigned char dummy[8];
	MPT_TEST_EXPECT_EQUAL(mpt::midpoint(&dummy[2], &dummy[3]), &dummy[2]);
	MPT_TEST_EXPECT_EQUAL(mpt::midpoint(&dummy[3], &dummy[2]), &dummy[3]);
}

} // namespace numeric
} // namespace base
} // namespace tests



} // namespace MPT_INLINE_NS
} // namespace mpt



#endif // MPT_BASE_TESTS_NUMERIC_HPP

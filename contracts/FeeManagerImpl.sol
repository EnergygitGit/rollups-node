// Copyright 2021 Cartesi Pte. Ltd.

// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License"); you may not use
// this file except in compliance with the License. You may obtain a copy of the
// License at http://www.apache.org/licenses/LICENSE-2.0

// Unless required by applicable law or agreed to in writing, software distributed
// under the License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR
// CONDITIONS OF ANY KIND, either express or implied. See the License for the
// specific language governing permissions and limitations under the License.

/// @title Fee Manager Impl
pragma solidity >=0.8.8;

import "./FeeManager.sol";
import "./ClaimsMaskImpl.sol";
import "./ValidatorManagerClaimsCountedImpl.sol";
import "@openzeppelin/contracts/token/ERC20/IERC20.sol";

// this FeeManagerImpl manages for up to 8 validators
contract FeeManagerImpl is FeeManager, ClaimsMaskImpl {
    ValidatorManagerClaimsCountedImpl ValidatorManagerCCI;
    uint256 feePerClaim;
    address owner;
    bool lock; //reentrancy lock

    /// @notice functions modified by onlyowner can only be accessed by contract owner
    modifier onlyOwner {
        require(owner == msg.sender, "only owner");
        _;
    }

    /// @notice functions modified by noReentrancy are not subject to recursion
    modifier noReentrancy() {
        require(!lock, "reentrancy not allowed");
        lock = true;
        _;
        lock = false;
    }

    /// @notice creates FeeManagerImpl contract
    /// @param _ValidatorManagerCCI address of ValidatorManagerClaimsCountedImpl
    constructor(address _ValidatorManagerCCI) {
        owner = msg.sender;
        ValidatorManagerCCI = ValidatorManagerClaimsCountedImpl(
            _ValidatorManagerCCI
        );
    }

    /// @notice this function can only be called by owner to deposit funds as rewards(fees) for validators
    /// @param _ERC20 address of ERC20 token to be deposited
    /// @param _amount amount of tokens to be deposited
    function erc20fund(address _ERC20, uint256 _amount)
        public
        override
        onlyOwner
    {
        IERC20 token = IERC20(_ERC20);
        require(
            token.transferFrom(owner, address(this), _amount),
            "erc20 fund deposit failed"
        );
        emit ERC20FundDeposited(_ERC20, _amount);
    }

    /// @notice contract owner can set/reset the value of fee per claim
    /// @param  _value the new value of fee per claim
    function setFeePerClaim(uint256 _value) public override onlyOwner {
        feePerClaim = _value;
    }

    /// @notice this function can be called to redeem fees for validators
    /// @param _ERC20 address of ERC20 token
    /// @param  _validator address of the validator that is redeeming
    function claimFee(address _ERC20, address _validator)
        public
        override
        noReentrancy
    {
        // follow the Checks-Effects-Interactions pattern for security

        // ** checks **
        uint256 valIndex = ValidatorManagerCCI.getValidatorIndex(_validator); // will revert if not found
        uint256 totalClaims =
            ValidatorManagerCCI.getNumberOfClaimsByIndex(valIndex);
        uint256 redeemedClaims = getNumClaimsRedeemed(valIndex);

        require(totalClaims > redeemedClaims, "nothing to redeem yet");

        // ** effects **
        uint256 nowRedeemingClaims = totalClaims - redeemedClaims;
        increaseNumClaimed(valIndex, nowRedeemingClaims);

        // ** interactions **
        IERC20 token = IERC20(_ERC20);
        uint256 feesToSend = nowRedeemingClaims * feePerClaim; // number of erc20 tokens to send
        require(token.transfer(_validator, feesToSend), "Failed to claim fees");

        emit FeeClaimed(_ERC20, _validator, feesToSend);
    }
}

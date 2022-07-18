import * as types from '../types';

export default function SolveResponse(props: {
    solveResponse: types.SolveResponse
    solutionLimit: number
}) {
    switch (props.solveResponse.ty) {
        case 'error':
            return <div>{props.solveResponse.message}</div>;
        case 'no-solution':
            return <div>No solution</div>;
        case 'solved':
            const solutionCount = props.solveResponse.response.solutions;
            if (solutionCount > props.solutionLimit) {
                return <div>More than {props.solutionLimit} solutions found</div>
            } else if (solutionCount > 1) {
                return <div>{solutionCount} solutions found</div>
            } else {
                return <div>{solutionCount} solution found</div>
            }
    }
}
